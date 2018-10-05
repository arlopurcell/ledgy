#![feature(plugin, custom_derive)]
#![plugin(rocket_codegen)]
extern crate rocket;
extern crate rocket_contrib;
#[macro_use] extern crate serde_derive;
extern crate rusqlite;
extern crate time;

use rusqlite::Connection;
use rocket_contrib::Json;

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
    transactions: Vec<TransResponse>,
    balance: i32
}

#[derive(FromForm)]
struct TimeRange {
    start: Option<String>,
    end: Option<String>
}

#[derive(FromForm)]
struct PagingArgs {
    page: Option<usize>,
    per_page: Option<usize>
}

#[get("/")]
fn index() -> &'static str {
    "Hello, world!"
}

#[post("/<ledger>/init")]
fn init(ledger: String) -> &'static str {
    let mut path_buf = get_path();
    path_buf.push(ledger);
    do_init(path_buf.as_path());
    "ok"
}

#[post("/<ledger>/credit", data="<trans>")]
fn credit(ledger: String, trans: Json<TransRequest>) -> &'static str {
    let mut path_buf = get_path();
    path_buf.push(ledger);
    let amount = trans.0.amount;
    do_transaction(path_buf.as_path(), amount, &trans.0.description);
    "ok"
}

#[post("/<ledger>/debit", data="<trans>")]
fn debit(ledger: String, trans: Json<TransRequest>) -> &'static str {
    let mut path_buf = get_path();
    path_buf.push(ledger);
    let amount = trans.0.amount * -1;
    do_transaction(path_buf.as_path(), amount, &trans.0.description);
    "ok"
}

#[get("/<ledger>")]
fn get_ledger(ledger: String) -> Json<TransList> {
    get_ledger_paged(ledger, PagingArgs{page: None, per_page: None})
}

#[get("/<ledger>/time_range?<time_range>")]
fn get_ledger_with_range(ledger: String, time_range: TimeRange) -> Json<TransList> {
    let mut path_buf = get_path();
    path_buf.push(ledger);
    let path = path_buf.as_path();
    let conn = Connection::open(path).unwrap();

    let base_stmt = "SELECT amount, balance, description, time_created FROM ledger";

    let mut clauses = Vec::new();
    if let Some(start) = time_range.start {
        clauses.push(format!("time_created > \"{}\"", start));
    }
    if let Some(end) = time_range.end {
        clauses.push(format!("time_created < \"{}\"", end));
    }
    let where_clause = format!(" where {}", clauses.join(" and "));

    let mut stmt = conn.prepare(&format!("{}{}", base_stmt, where_clause)).unwrap();
    let transactions = stmt.query_map(&[], |row| {
        TransResponse {
            amount: row.get(0),
            balance: row.get(1),
            description: row.get(2),
            time: row.get(3),
        }
    }).unwrap().map(|trans_result| {trans_result.unwrap()}).collect();
    let balance = get_balance(path, &where_clause);
    Json(TransList{
        transactions: transactions,
        balance: balance,
    })
}

#[get("/<ledger>?<paging>")]
fn get_ledger_paged(ledger: String, paging: PagingArgs) -> Json<TransList> {
    let mut path_buf = get_path();
    path_buf.push(ledger);
    let path = path_buf.as_path();
    let conn = Connection::open(path).unwrap();

    let page = paging.page.unwrap_or(0);
    let per_page = paging.per_page.unwrap_or(10);

    let mut stmt = conn.prepare("SELECT amount, balance, description, time_created FROM ledger ORDER BY time_created DESC").unwrap();
    let mut transactions = stmt.query_map(&[], |row| {
        TransResponse {
            amount: row.get(0),
            balance: row.get(1),
            description: row.get(2),
            time: row.get(3),
        }
    }).unwrap().map(|trans_result| {trans_result.unwrap()}).skip(page * per_page).take(per_page).peekable();
    let balance = match transactions.peek() {
        Some(last_trans) => last_trans.balance,
        None => 0
    };
    Json(TransList{
        transactions: transactions.collect(),
        balance: balance,
    })
}

#[derive(Serialize)]
struct LedgerList {
    ledgers: Vec<String>
}

#[get("/list")]
fn list_ledgers() -> Json<LedgerList> {
    let dir = get_path();
    let ledgers = dir.read_dir().unwrap().filter_map(|entry| {
        match entry {
            Ok(file) => file.file_name().into_string().ok(),
            _ => None
        }
    }).collect();
    Json(LedgerList { ledgers: ledgers })
}


fn get_path() -> PathBuf {
    // TODO get real path
    PathBuf::from("ledgers")
}

fn main() {
    rocket::ignite().mount("/ledger", 
            routes![
                index, 
                init,
                credit, 
                debit,
                get_ledger, 
                get_ledger_with_range, 
                get_ledger_paged, 
                list_ledgers
            ]).launch();
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
    let balance = get_balance(path, "");
    let conn = Connection::open(path).unwrap();
    conn.execute("INSERT INTO ledger (amount, balance, description, time_created)
                  VALUES (?1, ?2, ?3, ?4)",
                 &[&amount, &(balance + amount), &description, &time::get_time()]).unwrap();
}

fn get_balance(path: &Path, where_clause: &str) -> i32 {
    let conn = Connection::open(path).unwrap();
    let mut stmt = conn.prepare(&format!("SELECT balance FROM ledger {} ORDER BY time_created DESC LIMIT 1", where_clause)).unwrap();
    let mut balance_iter = stmt.query_map(&[], |row| {
        row.get(0)
    }).unwrap();
    match balance_iter.next() {
        Some(Ok(balance)) => balance,
        _ => 0,
    }
}
