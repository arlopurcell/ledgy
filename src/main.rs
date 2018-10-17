#![feature(plugin, custom_derive)]
#![plugin(rocket_codegen)]
extern crate rocket;
extern crate rocket_contrib;
#[macro_use] extern crate serde_derive;
extern crate rusqlite;
extern crate time;

use rusqlite::{Connection, OpenFlags};
use rocket_contrib::Json;
use rocket::response::NamedFile;
use rocket::request::State;
use rocket::fairing::AdHoc;

use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::sync::RwLock;


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

    conn.execute("CREATE TABLE ledger (
                  amount       INTEGER NOT NULL,
                  balance      INTEGER NOT NULL,
                  description  TEXT NOT NULL,
                  time_created INTEGER NOT NULL
                  )", &[]).unwrap();
    conn.execute("CREATE INDEX time_index on ledger(time_created)", &[]).unwrap();
    
    connections.map_lock.write().unwrap().insert(ledger, lock);
    "ok"
}

#[post("/<ledger>/credit", data="<trans>")]
fn credit(ledger: String, trans: Json<TransRequest>, connections: State<LedgerConnections>) -> &'static str {
    let amount = trans.0.amount;
    let conn = connections.get_write(&ledger);
    do_transaction(conn, amount, &trans.0.description);
    "ok"
}

#[post("/<ledger>/debit", data="<trans>")]
fn debit(ledger: String, trans: Json<TransRequest>, connections: State<LedgerConnections>) -> &'static str {
    let amount = trans.0.amount * -1;
    let conn = connections.get_write(&ledger);
    do_transaction(conn, amount, &trans.0.description);
    "ok"
}

#[post("/<ledger>/edit/<rowid>", data="<trans>")]
fn edit_trans(ledger: String, rowid: i32, trans: Json<TransRequest>, connections: State<LedgerConnections>) -> &'static str {
    let mut conn = connections.get_write(&ledger);
    let tx = conn.transaction().unwrap();
    let old_amount: i32 = tx.query_row(&format!("SELECT amount FROM ledger WHERE rowid = {}", rowid), &[], |row| {
        row.get(0)
    }).unwrap();
    let diff = old_amount - trans.0.amount;

    tx.execute("UPDATE ledger SET amount = ?1, description = ?2 WHERE rowid = ?3",
                 &[&trans.0.amount, &trans.0.description, &rowid]).unwrap();

    tx.execute("UPDATE ledger SET balance = balance - ?1 WHERE rowid >= ?2",
                &[&diff, &rowid]).unwrap();

    tx.commit().unwrap();
    
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
    let debits = debit_stmt.query_map(&[], |row| {
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
    let credits = credit_stmt.query_map(&[], |row| {
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
    map_lock: RwLock<HashMap<String, RwLock<ConnectionFactory>>>
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

fn main() {
    rocket::ignite()
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
               ])
        .attach(AdHoc::on_attach(|rocket| {
            let assets_dir = rocket.config()
                .get_str("data_dir")
                .unwrap_or("ledgers/")
                .to_string();
            Ok(rocket.manage(DataDir(assets_dir)))
        }))
        .attach(AdHoc::on_attach(|rocket| {
            let assets_dir = rocket.config()
                .get_str("data_dir")
                .unwrap_or("ledgers/")
                .to_string();
            let dir = Path::new(&assets_dir);
            let connections: HashMap<String, RwLock<ConnectionFactory>> = dir.read_dir().unwrap().filter_map(|entry| {
                match entry {
                    Ok(file) => Some((
                        file.file_name().into_string().unwrap(), 
                        RwLock::new(ConnectionFactory{path: file.path()})
                    )),
                    _ => None
                }
            }).collect();
            Ok(rocket.manage(LedgerConnections{map_lock: RwLock::new(connections)}))
        }))
        .launch();
}

fn do_transaction(conn: Connection, amount: i32, description: &str) {
    let balance = get_balance(&conn);
    conn.execute("INSERT INTO ledger (amount, balance, description, time_created)
                  VALUES (?1, ?2, ?3, ?4)",
                 &[&amount, &(balance + amount), &description, &time::get_time()]).unwrap();
}

fn get_balance(conn: &Connection) -> i32 {
    conn.query_row("SELECT balance FROM ledger ORDER BY time_created DESC LIMIT 1", &[], |row| {
        row.get(0)
    }).unwrap_or(0)
}

