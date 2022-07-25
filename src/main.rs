#[macro_use]
extern crate fstrings;

use std::fs;
use futures::stream::{StreamExt};
use indicatif::{ProgressBar, ProgressStyle};

use mongodb::{
    bson::DateTime,
    bson::doc,
    options::FindOptions,
    Client as mongo,
    Collection,
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
    let addresses: Vec<String> = fs::read_to_string("./addresses.txt").unwrap_or("".to_string()).split(LINE_ENDING).map(str::to_string).collect();

    let client: mongodb::Client = mongo::with_uri_str("mongodb://127.0.0.1").await.unwrap();
    let database = client.database("ronin");
    let collection: Collection<Transaction> = database.collection::<Transaction>("transactions");

    let find_options: FindOptions = mongodb::options::FindOptions::builder().limit(5000).build();
    let count_options = mongodb::options::CountOptions::builder().limit(5000).build();

    let http_client: Client = Client::new();

    let  mut final_output: Vec<AddressDict> = vec![];

    if addresses.len() > 0 {
        println!("Found {} addresses", addresses.len());

        for address in addresses {

            let mut address_output: AddressDict = AddressDict {
                address: address.clone(),
                tx: vec![]
            };

            let txs_num = &collection.count_documents(doc! {"from": address.clone().to_lowercase().trim()}, count_options.clone()).await.unwrap();
            let mut txs = collection.find(doc! {"from": address.clone().to_lowercase().trim()}, find_options.clone()).await.unwrap();

            let bar = ProgressBar::new(*txs_num);

            bar.set_style(
                ProgressStyle::default_spinner().template("{spinner}{bar:80.cyan/blue} {percent:>3}% | [{eta_precise}][{elapsed_precise}] ETA/Elapsed | {msg}{pos:>5}/{len:4}").unwrap()
            );

            println!("Processing {} with {} txs", &address.to_lowercase().trim(), txs_num);

            while let Some(tx) = txs.next().await {
                let tx = tx.unwrap().hash;
                bar.set_message(f!("Decoding: {}", tx.clone()));

                let decoded = get_tx(&http_client, tx).await;

                address_output.tx.push(decoded);
                bar.inc(1);
            }

            bar.set_position(*txs_num);
            bar.finish();

            final_output.push(address_output);

        }

        fs::write("output.json", serde_json::to_string(&final_output).unwrap()).ok();
        print!("Saved output to output.json!");

    } else {
        println!("No addresses found!")
    }
}
