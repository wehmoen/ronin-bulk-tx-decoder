#[macro_use]
extern crate fstrings;

use std::{fs, thread};
use std::time::Duration;

use futures::stream::StreamExt;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use mongodb::{
    bson::DateTime,
    bson::doc,
    Client as mongo,
    Collection,
    options::FindOptions,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const URL_BASE_DECODE_RECEIPT: &str = "http://localhost:3000/ronin/decodeTransactionReceipt/";
const URL_BASE_DECODE_INPUT: &str = "http://localhost:3000/ronin/decodeTransaction/";

#[cfg(windows)]
const LINE_ENDING: &'static str = "\r\n";
#[cfg(not(windows))]
const LINE_ENDING: &'static str = "\n";

#[derive(Debug, Serialize, Deserialize)]
struct DecodedTransaction {
    hash: String,
    input: Value,
    logs: Value,
}

#[derive(Debug, Serialize, Deserialize)]
struct AddressDict {
    address: String,
    tx: Vec<DecodedTransaction>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Transaction {
    from: String,
    to: String,
    hash: String,
    block: u32,
    created_at: DateTime,
}


async fn get_tx(client: &Client, hash: String) -> DecodedTransaction {
    let input: Value = serde_json::from_str(&*client.get(f!("{URL_BASE_DECODE_INPUT}{hash}"))
        .send()
        .await
        .unwrap()
        .text().await.unwrap()).unwrap();

    let logs: Value = serde_json::from_str(&*client.get(f!("{URL_BASE_DECODE_RECEIPT}{hash}"))
        .send()
        .await
        .unwrap()
        .text().await.unwrap()).unwrap();

    DecodedTransaction {
        hash,
        input,
        logs,
    }
}

#[tokio::main]
async fn main() {
    let multi = MultiProgress::new();
    let style = ProgressStyle::default_spinner().template("{spinner}{bar:80.cyan/blue} {percent:>3}% | [{eta_precise}][{elapsed_precise}] ETA/Elapsed | {msg}{pos:>5}/{len:4}").unwrap();

    let addresses: Vec<String> = fs::read_to_string("./addresses.txt").unwrap_or("".to_string()).split(LINE_ENDING).map(str::to_string).collect();

    fs::create_dir_all("./output").ok();

    let pb_global = multi.add(ProgressBar::new(addresses.len() as u64));
    pb_global.set_style(style.clone());

    let client: mongodb::Client = mongo::with_uri_str("mongodb://127.0.0.1").await.unwrap();
    let database = client.database("ronin");
    let collection: Collection<Transaction> = database.collection::<Transaction>("transactions");

    let find_options: FindOptions = mongodb::options::FindOptions::builder().limit(5000).build();
    let count_options = mongodb::options::CountOptions::builder().limit(5000).build();

    let http_client: Client = Client::new();

    let mut final_output: Vec<AddressDict> = vec![];

    if addresses.len() > 0 {
        println!("Found {} addresses:\n", addresses.len());

        for address in addresses {
            pb_global.set_message(format!("Processing {}", address.clone()));

            let mut address_output: AddressDict = AddressDict {
                address: address.clone(),
                tx: vec![],
            };

            let txs_num = &collection.count_documents(doc! {"from": address.clone().to_lowercase().trim()}, count_options.clone()).await.unwrap();

            let pb_task = multi.insert_before(&pb_global, ProgressBar::new(*txs_num));
            pb_task.set_style(style.clone());

            let mut txs = collection.find(doc! {"from": address.clone().to_lowercase().trim()}, find_options.clone()).await.unwrap();

            while let Some(tx) = txs.next().await {
                let tx = tx.unwrap().hash;
                pb_task.set_message(format!("Decoding: {}", tx.clone()));

                let decoded = get_tx(&http_client, tx).await;

                address_output.tx.push(decoded);
                pb_task.inc(1);
            }

            pb_task.set_position(*txs_num);
            pb_task.finish();

            fs::write(f!("./output/{address}.json"), serde_json::to_string(&address_output).unwrap()).ok();

            pb_global.inc(1);
        }

        print!("Saved output to ./output!");
    } else {
        println!("No addresses found!")
    }

}
