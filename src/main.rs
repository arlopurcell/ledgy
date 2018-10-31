#![feature(plugin, custom_derive)]
#![plugin(rocket_codegen)]
extern crate rocket;
extern crate rocket_contrib;
#[macro_use] extern crate serde_derive;
extern crate rusqlite;
extern crate chrono;

use rusqlite::{Connection, OpenFlags, NO_PARAMS};
use rusqlite::types::ToSql;
use rocket_contrib::Json;
use rocket::response::NamedFile;
use rocket::request::State;
use rocket::fairing::AdHoc;
use chrono::{Local, DateTime, Weekday, Datelike};

use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;


#[derive(Deserialize)]
struct TransRequest {
    amount: i32,
    description: String
}

#[derive(Serialize)]
struct TransResponse {
    rowid: i32,
    amount: i32,
    balance: i32,
    description: String,
    time: String
}

#[derive(Serialize)]
struct TransList {
    debits: Vec<TransResponse>,
    credits: Vec<TransResponse>,
    balance: i32
}

#[derive(FromForm)]
struct PagingArgs {
    page: Option<usize>,
    per_page: Option<usize>,
}

#[post("/<ledger>/init")]
fn init(ledger: String, data_dir: State<DataDir>, connections: State<LedgerConnections>) -> &'static str {
    let mut path_buf = PathBuf::from(&data_dir.0);
    path_buf.push(ledger.clone());

    let lock = RwLock::new(ConnectionFactory{path: path_buf});
    let conn = lock.write().unwrap().get_write();

    conn.execute("CREATE TABLE IF NOT EXISTS ledger (
                  amount       INTEGER NOT NULL,
                  balance      INTEGER NOT NULL,
                  description  TEXT NOT NULL,
                  time_created INTEGER NOT NULL
                  )", NO_PARAMS).unwrap();
    conn.execute("CREATE INDEX IF NOT EXISTS time_index on ledger(time_created)", NO_PARAMS).unwrap();

    conn.execute("CREATE TABLE IF NOT EXISTS cron_last_run (
                  date_time TEXT NOT NULL
            )", NO_PARAMS).unwrap();

    conn.execute("CREATE TABLE IF NOT EXISTS crons (
                  type         TEXT NOT NULL,
                  idx        INTEGER NOT NULL,
                  amount       INTEGER NOT NULL,
                  description  TEXT NOT NULL
            )", NO_PARAMS).unwrap();
    
    connections.map_lock.write().unwrap().insert(ledger, lock);
    "ok"
}

#[post("/<ledger>/credit", data="<trans>")]
fn credit(ledger: String, trans: Json<TransRequest>, connections: State<LedgerConnections>) -> &'static str {
    let amount = trans.0.amount;
    let conn = connections.get_write(&ledger);
    do_transaction(&conn, amount, &trans.0.description);
    "ok"
}

#[post("/<ledger>/debit", data="<trans>")]
fn debit(ledger: String, trans: Json<TransRequest>, connections: State<LedgerConnections>) -> &'static str {
    let amount = trans.0.amount * -1;
    let conn = connections.get_write(&ledger);
    do_transaction(&conn, amount, &trans.0.description);
    "ok"
}

#[post("/<ledger>/edit/<rowid>", data="<trans>")]
fn edit_trans(ledger: String, rowid: i32, trans: Json<TransRequest>, connections: State<LedgerConnections>) -> &'static str {
    let mut conn = connections.get_write(&ledger);
    let tx = conn.transaction().unwrap();
    let old_amount: i32 = tx.query_row(&format!("SELECT amount FROM ledger WHERE rowid = {}", rowid), NO_PARAMS, |row| {
        row.get(0)
    }).unwrap();
    let diff = old_amount - trans.0.amount;

    tx.execute("UPDATE ledger SET amount = ?1, description = ?2 WHERE rowid = ?3",
                 &[&trans.0.amount as &ToSql, &trans.0.description, &rowid]).unwrap();

    tx.execute("UPDATE ledger SET balance = balance - ?1 WHERE rowid >= ?2",
                &[&diff, &rowid]).unwrap();

    tx.commit().unwrap();
    
    "ok"
}

#[post("/<ledger>/cron", data="<cron>")]
fn create_cron(ledger: String, cron: Json<CronSpec>, connections: State<LedgerConnections>) -> &'static str {
    let conn = connections.get_write(&ledger);
    let (cron_type, index) = cron.0.schedule.to_sql();
    conn.execute("INSERT INTO crons (type, idx, amount, description)
                  VALUES (?1, ?2, ?3, ?4)",
                 &[&cron_type as &ToSql, &index, &cron.0.amount, &cron.0.description]).unwrap();
    "ok"
}

#[derive(Serialize)]
struct CronResponse {
    rowid: i32,
    spec: CronSpec,
}

#[derive(Serialize)]
struct CronList {
    crons: Vec<CronResponse>
}

#[get("/<ledger>/crons")]
fn get_crons(ledger: String, connections: State<LedgerConnections>) -> Json<CronList> {
    let conn = connections.get_write(&ledger);
    let mut cron_stmt = conn.prepare(
        "SELECT rowid, type, idx, amount, description FROM crons"
        ).unwrap();
    let crons: Vec<CronResponse> = cron_stmt.query_map(NO_PARAMS, |row| {
        let cron_type: String = row.get(1);
        let spec = CronSpec {
            schedule: CronSchedule::from_sql(&cron_type, row.get(2)).unwrap(),
            amount: row.get(3),
            description: row.get(4),
        };
        CronResponse { rowid: row.get(0), spec }
    }).unwrap().map(|result| {result.unwrap()}).collect();
    Json(CronList {crons:crons})
}

// TODO edit cron (takes rowid)
#[delete("/<ledger>/cron/<rowid>")]
fn delete_cron(ledger:String, rowid: i32, connections: State<LedgerConnections>) -> &'static str {
    let conn = connections.get_write(&ledger);
    conn.execute("DELETE FROM crons WHERE rowid = ?1", &[&rowid]).unwrap();
    "ok"
}

#[get("/<ledger>")]
fn get_ledger(ledger: String, connections: State<LedgerConnections>) -> Json<TransList> {
    get_ledger_paged(ledger, PagingArgs{page: None, per_page: None}, connections)
}

#[get("/<ledger>?<paging>")]
fn get_ledger_paged(ledger: String, paging: PagingArgs, connections:State<LedgerConnections>) -> Json<TransList> {
    let conn = connections.get_read(&ledger);
    let page = paging.page.unwrap_or(0);
    let per_page = paging.per_page.unwrap_or(20);

    let mut debit_stmt = conn.prepare(
        "SELECT amount, balance, description, time_created, rowid FROM ledger WHERE amount < 0 ORDER BY time_created DESC"
    ).unwrap();
    let debits = debit_stmt.query_map(NO_PARAMS, |row| {
        TransResponse {
            amount: row.get(0),
            balance: row.get(1),
            description: row.get(2),
            time: row.get(3),
            rowid: row.get(4),
        }
    }).unwrap().map(|trans_result| {trans_result.unwrap()}).skip(page * per_page).take(per_page);

    let mut credit_stmt = conn.prepare(
        "SELECT amount, balance, description, time_created, rowid FROM ledger WHERE amount > 0 ORDER BY time_created DESC"
    ).unwrap();
    let credits = credit_stmt.query_map(NO_PARAMS, |row| {
        TransResponse {
            amount: row.get(0),
            balance: row.get(1),
            description: row.get(2),
            time: row.get(3),
            rowid: row.get(4),
        }
    }).unwrap().map(|trans_result| {trans_result.unwrap()}).skip(page * per_page).take(per_page);

    let balance = get_balance(&conn);

    Json(TransList{
        debits: debits.collect(),
        credits: credits.collect(),
        balance: balance,
    })
}

#[derive(Serialize)]
struct LedgerList {
    ledgers: Vec<String>
}

#[get("/list")]
fn list_ledgers(connections: State<LedgerConnections>) -> Json<LedgerList> {
    let mut ledgers: Vec<String> = connections.map_lock.read().unwrap().keys().map(|s| {s.clone()}).collect();
    ledgers.sort_unstable();
    Json(LedgerList { ledgers: ledgers })
}

#[get("/static/<file..>")]
fn static_file(file: PathBuf) -> Option<NamedFile> {
    NamedFile::open(Path::new("static/").join(file)).ok()
}

#[get("/")]
fn index() -> Option<NamedFile> {
    NamedFile::open(Path::new("static/index.html")).ok()
}

#[get("/service-worker.js")]
fn service_worker() -> Option<NamedFile> {
    NamedFile::open(Path::new("static/service-worker.js")).ok()
}

struct DataDir(String);
struct LedgerConnections {
    map_lock: Arc<RwLock<HashMap<String, RwLock<ConnectionFactory>>>>
}

impl LedgerConnections {
    fn get_write(&self, ledger: &str) -> Connection {
        let map = self.map_lock.read().unwrap();
        let conn = map.get(ledger).unwrap().write().unwrap().get_write();
        conn
    }

    fn get_read(&self, ledger: &str) -> Connection {
        let map = self.map_lock.read().unwrap();
        let conn = map.get(ledger).unwrap().read().unwrap().get_read();
        conn
    }
}

struct ConnectionFactory {
    path: PathBuf,
}

impl ConnectionFactory {
    fn get_write(&mut self) -> Connection {
        Connection::open(&self.path).unwrap()
    }

    fn get_read(&self) -> Connection {
        Connection::open_with_flags(&self.path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
    }
}

fn do_transaction(conn: &Connection, amount: i32, description: &str) {
    let balance = get_balance(&conn);
    conn.execute("INSERT INTO ledger (amount, balance, description, time_created)
                  VALUES (?1, ?2, ?3, ?4)",
                 &[&amount as &ToSql, &(balance + amount), &description, &Local::now()]).unwrap();
}

fn get_balance(conn: &Connection) -> i32 {
    conn.query_row("SELECT balance FROM ledger ORDER BY time_created DESC LIMIT 1", NO_PARAMS, |row| {
        row.get(0)
    }).unwrap_or(0)
}

#[derive(Serialize, Deserialize)]
enum CronSchedule {
    Weekly(Weekday),
    Monthly(u32),
}

impl CronSchedule {
    fn matches<T: chrono::TimeZone>(&self, dt: &DateTime<T>) -> bool {
        match self {
            CronSchedule::Weekly(weekday) => dt.weekday() == *weekday,
            CronSchedule::Monthly(day) => dt.day() == *day,
        }
    }

    fn from_sql(cron_type: &str, index: u32) -> Result<CronSchedule, String> {
        match cron_type {
            "weekly" => match index {
                1 => Ok(CronSchedule::Weekly(Weekday::Mon)),
                2 => Ok(CronSchedule::Weekly(Weekday::Tue)),
                3 => Ok(CronSchedule::Weekly(Weekday::Wed)),
                4 => Ok(CronSchedule::Weekly(Weekday::Thu)),
                5 => Ok(CronSchedule::Weekly(Weekday::Fri)),
                6 => Ok(CronSchedule::Weekly(Weekday::Sat)),
                7 => Ok(CronSchedule::Weekly(Weekday::Sun)),
                _ => Err("Invalid weekday index".to_string()),
            },
            "monthly" => Ok(CronSchedule::Monthly(index)),
            _ => Err("Invalid schedule type".to_string())
        }
    }

    fn to_sql(&self) -> (&'static str, u32) {
        match self {
            CronSchedule::Weekly(weekday) => ("weekly", weekday.number_from_monday()),
            CronSchedule::Monthly(day) => ("monthly", *day),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct CronSpec {
    schedule: CronSchedule,
    amount: i32,
    description: String,
}

fn main() {
    let rocket = rocket::ignite()
        .mount("/", 
            routes![
                static_file,
                index,
                service_worker,
            ])
        .mount("/api",
            routes![
                init,
                credit, 
                debit,
                get_ledger, 
                get_ledger_paged, 
                list_ledgers,
                edit_trans,
                create_cron,
                get_crons,
                delete_cron,
               ])
        .attach(AdHoc::on_attach(|rocket| {
            let assets_dir = rocket.config()
                .get_str("data_dir")
                .unwrap_or("ledgers/")
                .to_string();
            Ok(rocket.manage(DataDir(assets_dir)))
        }));

    let connections: HashMap<String, RwLock<ConnectionFactory>> = {
        let data_dir: &DataDir = rocket.state().unwrap();
        let dir = Path::new(&data_dir.0);
        dir.read_dir().unwrap().filter_map(|entry| {
            match entry {
                Ok(file) => Some((
                        file.file_name().into_string().unwrap(), 
                        RwLock::new(ConnectionFactory{path: file.path()})
                        )),
                        _ => None
            }
        }).collect()
    };
    let super_lock = Arc::new(RwLock::new(connections));
    let cron_lock = Arc::clone(&super_lock);

    thread::spawn(move || {
        let six_hours = Duration::from_secs(6 * 60 * 60);
        loop {
            let now = Local::now();
            for conn_lock in cron_lock.read().unwrap().values() {
                let conn = conn_lock.write().unwrap().get_write();
                let last_run_opt: Option<DateTime<Local>> = conn.query_row("SELECT date_time from cron_last_run", NO_PARAMS, 
                                                                           |row| {row.get(0)}
                                                                          ).ok();
                if last_run_opt.is_none() {
                    continue;
                }
                let mut cron_stmt = conn.prepare(
                    "SELECT type, idx, amount, description FROM crons"
                    ).unwrap();
                let crons = cron_stmt.query_map(NO_PARAMS, |row| {
                    let cron_type: String = row.get(0);
                    CronSpec {
                        schedule: CronSchedule::from_sql(&cron_type, row.get(1)).unwrap(),
                        amount: row.get(2),
                        description: row.get(3),
                    }
                }).unwrap();
                for cron in crons {
                    let cron = cron.unwrap();
                    if cron.schedule.matches(&now) {
                        if !cron.schedule.matches(&last_run_opt.unwrap()) {
                            println!("Didn't match last");
                            do_transaction(&conn, cron.amount, &cron.description);
                        }
                    }
                }
                conn.execute("REPLACE INTO cron_last_run (rowid, date_time) VALUES (1, ?1)", &[&now]).unwrap();
            }
            thread::sleep(six_hours);
        }
    });
    rocket.attach(AdHoc::on_attach(move |rocket| {
        Ok(rocket.manage(LedgerConnections{map_lock: Arc::clone(&super_lock)}))
    })).launch();
}

