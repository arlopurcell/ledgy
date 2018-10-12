#![feature(plugin, custom_derive)]
#![plugin(rocket_codegen)]
extern crate rocket;
extern crate rocket_contrib;
#[macro_use] extern crate serde_derive;
extern crate rusqlite;
extern crate time;

use rusqlite::Connection;
use rocket_contrib::Json;
use rocket::response::NamedFile;
use rocket::request::State;
use rocket::fairing::AdHoc;

use std::path::{Path, PathBuf};

#[derive(Deserialize)]
struct TransRequest {
    amount: i32,
    description: String
}

#[derive(Serialize)]
struct TransResponse {
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
fn init(ledger: String, data_dir: State<DataDir>) -> &'static str {
    let mut path_buf = PathBuf::from(&data_dir.0);
    path_buf.push(ledger);
    do_init(path_buf.as_path());
    "ok"
}

#[post("/<ledger>/credit", data="<trans>")]
fn credit(ledger: String, trans: Json<TransRequest>, data_dir: State<DataDir>) -> &'static str {
    let mut path_buf = PathBuf::from(&data_dir.0);
    path_buf.push(ledger);
    let amount = trans.0.amount;
    do_transaction(path_buf.as_path(), amount, &trans.0.description);
    "ok"
}

#[post("/<ledger>/debit", data="<trans>")]
fn debit(ledger: String, trans: Json<TransRequest>, data_dir: State<DataDir>) -> &'static str {
    let mut path_buf = PathBuf::from(&data_dir.0);
    path_buf.push(ledger);
    let amount = trans.0.amount * -1;
    do_transaction(path_buf.as_path(), amount, &trans.0.description);
    "ok"
}

#[get("/<ledger>")]
fn get_ledger(ledger: String, data_dir: State<DataDir>) -> Json<TransList> {
    get_ledger_paged(ledger, PagingArgs{page: None, per_page: None}, data_dir)
}

#[get("/<ledger>?<paging>")]
fn get_ledger_paged(ledger: String, paging: PagingArgs, data_dir: State<DataDir>) -> Json<TransList> {
    let mut path_buf = PathBuf::from(&data_dir.0);
    path_buf.push(ledger);
    let path = path_buf.as_path();
    let conn = Connection::open(path).unwrap();

    let page = paging.page.unwrap_or(0);
    let per_page = paging.per_page.unwrap_or(10);

    let mut debit_stmt = conn.prepare(
        "SELECT amount, balance, description, time_created FROM ledger WHERE amount < 0 ORDER BY time_created DESC"
    ).unwrap();
    let debits = debit_stmt.query_map(&[], |row| {
        TransResponse {
            amount: row.get(0),
            balance: row.get(1),
            description: row.get(2),
            time: row.get(3),
        }
    }).unwrap().map(|trans_result| {trans_result.unwrap()}).skip(page * per_page).take(per_page);

    let mut credit_stmt = conn.prepare(
        "SELECT amount, balance, description, time_created FROM ledger WHERE amount > 0 ORDER BY time_created DESC"
    ).unwrap();
    let credits = credit_stmt.query_map(&[], |row| {
        TransResponse {
            amount: row.get(0),
            balance: row.get(1),
            description: row.get(2),
            time: row.get(3),
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
fn list_ledgers(data_dir: State<DataDir>) -> Json<LedgerList> {
    let dir = PathBuf::from(&data_dir.0);
    let ledgers = dir.read_dir().unwrap().filter_map(|entry| {
        match entry {
            Ok(file) => file.file_name().into_string().ok(),
            _ => None
        }
    }).collect();
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
               ])
        .attach(AdHoc::on_attach(|rocket| {
            let assets_dir = rocket.config()
                .get_str("data_dir")
                .unwrap_or("ledgers/")
                .to_string();
            Ok(rocket.manage(DataDir(assets_dir)))
        }))
        .launch();
}

fn do_init(path: &Path) {
    let conn = Connection::open(path).unwrap();
    conn.execute("CREATE TABLE ledger (
                  amount       INTEGER NOT NULL,
                  balance      INTEGER NOT NULL,
                  description  TEXT NOT NULL,
                  time_created INTEGER NOT NULL
                  )", &[]).unwrap();
    conn.execute("CREATE INDEX time_index on ledger(time_created)", &[]).unwrap();
}

fn do_transaction(path: &Path, amount: i32, description: &str) {
    let conn = Connection::open(path).unwrap();
    let balance = get_balance(&conn);
    conn.execute("INSERT INTO ledger (amount, balance, description, time_created)
                  VALUES (?1, ?2, ?3, ?4)",
                 &[&amount, &(balance + amount), &description, &time::get_time()]).unwrap();
}

fn get_balance(conn: &Connection) -> i32 {
    let mut stmt = conn.prepare("SELECT balance FROM ledger ORDER BY time_created DESC LIMIT 1").unwrap();
    let mut balance_iter = stmt.query_map(&[], |row| {
        row.get(0)
    }).unwrap();
    match balance_iter.next() {
        Some(Ok(balance)) => balance,
        _ => 0,
    }
}
