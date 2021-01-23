use std::io::{self, BufRead};
use std::fs::{File};
use regex::Regex;
use reqwest as request;
use reqwest::header::{USER_AGENT};
use std::ops::Add;

#[tokio::main]
async fn main() -> Result<(), request::Error> {
	let client = request::Client::new();
	let sessid = get_session_id(&client).await?;
	let results = get_listings(&client, &sessid).await?;
	Ok(())
}

async fn get_listings(client: &request::Client, id: &str) -> Result<(), request::Error> {
	let params = get_params();
	Ok(())
}

async fn get_session_id(client: &request::Client) -> Result<String, request::Error> {
	let re = Regex::new(r#"(PHPSESSID=[\w\S]+);"#).unwrap();
	let res = client
		.get("https://www.utahrealestate.com/index/public.index")
		.header(USER_AGENT, "reqwest program 0.1")
		.send()
		.await?;
	let sessid = res.headers().get("set-cookie").unwrap().to_str().unwrap();
	let mut id= String::from("");

	for cap in re.captures_iter(sessid) {
		id = String::from(&cap[1]);
	}

	if id == "" {
		panic!("unable to find sessid");
	}

	Ok(id)
}

fn get_params() {
	let mut paramEncoded = String::from("");
	if let Ok(lines) = read_lines("./params") {
		for line in lines {
			if let Ok(l) = line {
				paramEncoded.push_str(&format!("{}&", l));
			}
		}
	}
	println!("paramEncoded: {}", paramEncoded);
}

fn read_lines(filename: &str) -> io::Result<io::Lines<io::BufReader<File>>> {
	let file = File::open(filename)?;
	Ok(io::BufReader::new(file).lines())
}