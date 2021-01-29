#[macro_use]
extern crate log;
extern crate simplelog;

use futures::future;
use futures::future::{BoxFuture, FutureExt};
use reqwest as request;

use base64::encode;
use dirs::home_dir;
use futures::io::SeekFrom;
use regex::Regex;
use request::header::{HeaderMap, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use simplelog::*;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, LineWriter, Seek, Write};
use std::str;

const SURE_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_6) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/14.0.2 Safari/605.1.15";
const TWILIO_BASE_URL: &str = "https://api.twilio.com/2010-04-01";

#[tokio::main]
async fn main() -> SureResult<()> {
	init_logging()?;

	let client = request::Client::new();
	let sess_id = get_session_id(&client).await?;
	let mut listings = get_listings(&client, &sess_id, 0).await?;
	remove_duplicates(&mut listings);
	if listings.markers.len() > 0 {
		let listings_map = scrape_listings(&client, &listings).await?;
		let desired_listings = parse_listings(&listings_map);
		if desired_listings.len() > 0 {
			let listing_message = build_listing_message(&desired_listings);
			send_messages(&client, &listing_message).await?;
		}
	}

	Ok(())
}

fn init_logging() -> SureResult<()> {
	let log_file = OpenOptions::new()
		.append(true)
		.create(true)
		.open(&get_sure_filepath("sure.log"))?;
	let config = ConfigBuilder::new()
		.set_time_format_str("%c")
		.set_time_to_local(true)
		.build();

	CombinedLogger::init(vec![WriteLogger::new(LevelFilter::Info, config, log_file)]).unwrap();

	Ok(())
}

async fn get_session_id(client: &request::Client) -> SureResult<String> {
	let re = Regex::new(r#"(PHPSESSID=[\w\S]+);"#).unwrap();

	let res = client
		.get("https://www.utahrealestate.com/index/public.index")
		.header(USER_AGENT, SURE_USER_AGENT)
		.send()
		.await?;
	let sessid = res.headers().get("set-cookie").unwrap().to_str().unwrap();
	let mut id = String::from("");

	for cap in re.captures_iter(sessid) {
		id = String::from(&cap[1]);
	}

	if id == "" {
		panic!("unable to find session id");
	}

	Ok(id)
}

fn get_listings<'a>(
	client: &'a request::Client,
	session_id: &'a str,
	retry_count: usize,
) -> BoxFuture<'a, SureResult<UreData>> {
	if retry_count > 3 {
		error!("exceeded retry count - URE must be down");
		std::process::exit(0);
	}
	async move {
		let params = get_ure_search_params();
		let mut headers = HeaderMap::new();
		headers.insert(USER_AGENT, SURE_USER_AGENT.parse().unwrap());
		headers.insert(
			CONTENT_TYPE,
			"application/x-www-form-urlencoded".parse().unwrap(),
		);
		headers.insert("PHPSESSID", session_id.parse().unwrap());
		let res = client
			.post("https://www.utahrealestate.com/search/chained.update/param_reset/county_code,o_county_code,city,o_city,zip,o_zip,geometry,o_geometry/count/false/criteria/false/pg/1/limit/50/dh/1190")
			.headers(headers)
			.body(params)
			.send()
			.await?;

		let res_text = res.text().await?;
		match serde_json::from_str(&res_text) {
			Ok(v) => Ok(v),
			Err(_) => {
				error!("failed to parse text, retrying");
				Ok(get_listings(client, session_id, retry_count + 1).await?)
			}
		}
	}.boxed()
}

async fn scrape_listings(
	client: &request::Client,
	data: &UreData,
) -> SureResult<HashMap<String, Html>> {
	let mut raw_futures = vec![];
	for (index, marker) in data.markers.iter().enumerate() {
		raw_futures.push(get_listing(&client, &marker.id, index));
	}
	let unpin_futures: Vec<_> = raw_futures.into_iter().map(Box::pin).collect();
	let mut mut_futures = unpin_futures;

	let mut documents: HashMap<String, Html> = HashMap::new();
	let mut size: usize = 0;

	let mut current: f32 = 0.0;
	let total: usize = mut_futures.len();

	while !mut_futures.is_empty() {
		match future::select_all(mut_futures).await {
			(Ok((id, _idx, document, content_length)), _index, remaining) => {
				current += 1.0;
				let percentage = (((current / total as f32) * 100.0) / 2.0) as usize;
				io::stdout()
					.write(
						format!(
							"\rdownloading listings {}/{}: [{}>{}]",
							current,
							total,
							"=".repeat(percentage),
							" ".repeat(50 - percentage),
						)
						.as_bytes(),
					)
					.unwrap();
				io::stdout().flush().unwrap();
				size += content_length;
				documents.insert(id, document);
				mut_futures = remaining;
			}
			(Err(_e), _index, remaining) => {
				error!("document failed");
				mut_futures = remaining;
			}
		}
	}

	println!("\n");

	info!(
		"downloaded {:.2?}MB from {} listings{}",
		size as f32 / 1000000.0,
		total,
		" ".repeat(50)
	);

	Ok(documents)
}

fn parse_listings(listing_map: &HashMap<String, Html>) -> Vec<DesiredListing> {
	let selector = Selector::parse("div.fact-copy-wrap").unwrap();
	let mut desired_listings: Vec<DesiredListing> = vec![];
	for (key, value) in listing_map {
		let mut dl = DesiredListing::new();
		let div = value.select(&selector).collect::<Vec<_>>();
		for node in div {
			let mut node_vec = node
				.text()
				.collect::<Vec<&str>>()
				.iter()
				.map(|&v| v.trim())
				.collect::<Vec<&str>>();
			node_vec.remove(0);
			if node_vec[0] == "Days on URE" && node_vec[1] == "Just Listed" {
				dl.interested = true;
			}
			if node_vec[0] == "Days on URE"
				&& node_vec[1].to_string().parse::<usize>().unwrap() >= 20
			{
				dl.interested = true;
			}
			if node_vec[0] == "Status" && node_vec[1] == "Active" {
				dl.active = true;
			}
		}

		if dl.is_desired() {
			dl.mls = String::from(key);
			desired_listings.push(dl);
		}
	}

	desired_listings
}

fn remove_duplicates(listings: &mut UreData) {
	let mut dup_idx: Vec<usize> = vec![];
	let mut existing = get_checked_listings();
	for (idx, listing) in listings.markers.iter().enumerate() {
		if existing.contains(&listing.id) {
			dup_idx.push(idx);
		}
	}

	if dup_idx.len() > 0 {
		for i in dup_idx.into_iter().rev() {
			listings.markers.remove(i);
		}
	}

	if listings.markers.len() > 0 {
		for listing in listings.markers.iter() {
			existing.push(listing.id.clone());
		}
		write_checked_listings(&existing).unwrap();
	} else {
		info!("no new listings");
	}
}

fn build_listing_message(listings: &Vec<DesiredListing>) -> String {
	let mut message_str = String::from("");

	for listing in listings {
		message_str.push_str(&format!(
			"https://www.utahrealestate.com/{}\n\n",
			listing.mls
		));
	}

	message_str
}

async fn send_messages(client: &request::Client, message: &str) -> SureResult<()> {
	let credentials = get_twilio_credentials();
	let mut raw_futures = vec![];
	for number in credentials.alert_numbers.iter() {
		raw_futures.push(send_message(&client, &message, number))
	}

	let unpin_futures: Vec<_> = raw_futures.into_iter().map(Box::pin).collect();
	let mut mut_futures = unpin_futures;

	while !mut_futures.is_empty() {
		match future::select_all(mut_futures).await {
			(Ok(_res), _index, remaining) => mut_futures = remaining,
			(Err(_e), _index, remaining) => mut_futures = remaining,
		}
	}

	Ok(())
}

async fn get_listing(
	client: &request::Client,
	id: &str,
	index: usize,
) -> SureResult<(String, usize, Html, usize)> {
	let url = format!("https://www.utahrealestate.com/{}", id);
	let res = client
		.get(&url)
		.header(USER_AGENT, SURE_USER_AGENT)
		.send()
		.await?;

	let body = res.text().await?;
	let document = Html::parse_document(&body);

	Ok((String::from(id), index, document, body.len()))
}

async fn send_message(client: &request::Client, message: &str, to: &str) -> SureResult<()> {
	let credentials = get_twilio_credentials();
	let message_url = format!(
		"{}/Accounts/{}/Messages.json",
		TWILIO_BASE_URL, credentials.sid
	);
	let mut headers = HeaderMap::new();
	headers.insert(
		AUTHORIZATION,
		format!("Basic {}", credentials.basic_auth())
			.parse()
			.unwrap(),
	);

	let params = [
		("From", &credentials.number),
		("Body", &message.to_string()),
		("To", &to.to_string()),
	];
	let res = client
		.post(&message_url)
		.headers(headers)
		.form(&params)
		.send()
		.await?;

	if res.status() == 201 {
		info!("message sent");
	} else {
		error!(
			"error sending message: {:?}\n└──{}\n└──{:?}",
			res.status(),
			res.text().await?,
			params
		)
	}

	Ok(())
}

///
/// Utility Functions
///

fn get_checked_listings() -> Vec<String> {
	let mut checked_mls: Vec<String> = vec![];
	if let Ok(lines) = read_lines(&get_sure_filepath("listings.txt")) {
		for line in lines {
			if let Ok(l) = line {
				checked_mls.push(String::from(l.trim()))
			}
		}
	}

	checked_mls
}

fn write_checked_listings(checked: &Vec<String>) -> SureResult<()> {
	let mut contents = String::from("");
	let mut file = OpenOptions::new()
		.write(true)
		.create(true)
		.open(&get_sure_filepath("listings.txt"))?;

	file.set_len(0)?;
	file.seek(SeekFrom::Start(0))?;

	let mut file = LineWriter::new(file);

	for c in checked {
		contents.push_str(&format!("{}\n", c));
	}

	file.write_all(contents.as_bytes())?;

	Ok(())
}

fn get_ure_search_params() -> String {
	let mut param_encoded = String::from("");
	if let Ok(lines) = read_lines(&get_sure_filepath("queries.env")) {
		for line in lines {
			if let Ok(l) = line {
				param_encoded.push_str(&format!("{}&", l));
			}
		}
	}
	String::from(param_encoded)
}

fn get_twilio_credentials() -> TwilioAuth {
	let mut auth = TwilioAuth::new();
	if let Ok(lines) = read_lines(&get_sure_filepath("twilio.env")) {
		for line in lines {
			if let Ok(i) = line {
				let config_item: Vec<&str> = i.split('=').collect();
				if config_item[0] == "AccountSID" {
					auth.sid = String::from(config_item[1]);
				}
				if config_item[0] == "AuthToken" {
					auth.auth_token = String::from(config_item[1]);
				}
				if config_item[0] == "TwilioNumber" {
					auth.number = String::from(config_item[1]);
				}
				if config_item[0] == "AlertNumbers" {
					let numbers: Vec<String> = config_item[1]
						.split(",")
						.into_iter()
						.map(String::from)
						.collect();
					auth.alert_numbers = numbers;
				}
			}
		}
	}
	auth
}

fn read_lines(filename: &str) -> io::Result<io::Lines<io::BufReader<File>>> {
	let file = File::open(filename)?;
	Ok(io::BufReader::new(file).lines())
}

fn get_sure_filepath(filename: &str) -> String {
	let mut home_path = home_dir().unwrap();
	home_path.push(".sure");
	home_path.push(filename);
	String::from(home_path.to_str().unwrap())
}

///
///
/// Definitions and Implementations
///
///

///
/// DesiredListing
///
#[derive(Debug)]
struct DesiredListing {
	active: bool,
	interested: bool,
	mls: String,
}

impl DesiredListing {
	fn new() -> DesiredListing {
		Default::default()
	}

	fn is_desired(&self) -> bool {
		self.active && self.interested
	}
}

impl Default for DesiredListing {
	fn default() -> Self {
		DesiredListing {
			active: false,
			interested: false,
			mls: String::from(""),
		}
	}
}

///
/// Twilio
///
pub struct TwilioAuth {
	sid: String,
	auth_token: String,
	number: String,
	alert_numbers: Vec<String>,
}

impl TwilioAuth {
	fn new() -> TwilioAuth {
		Default::default()
	}

	fn basic_auth(&self) -> String {
		encode(format!("{}:{}", &self.sid, &self.auth_token).as_bytes())
	}
}

impl Default for TwilioAuth {
	fn default() -> Self {
		TwilioAuth {
			sid: String::from(""),
			auth_token: String::from(""),
			number: String::from(""),
			alert_numbers: vec![],
		}
	}
}

#[derive(Debug, Serialize, Deserialize)]
struct TwilioResponse {
	error_code: String,
	status: String,
}

///
/// SureResult and SureError
///
type SureResult<T> = Result<T, SureError>;

#[derive(Debug)]
enum SureError {
	IoError(std::io::Error),
	ReqwestError(request::Error),
	StdError(Box<dyn std::error::Error>),
	JsonError(serde_json::Error),
}

impl From<std::io::Error> for SureError {
	fn from(error: std::io::Error) -> Self {
		SureError::IoError(error)
	}
}

impl From<reqwest::Error> for SureError {
	fn from(error: reqwest::Error) -> Self {
		SureError::ReqwestError(error)
	}
}

impl From<Box<dyn std::error::Error>> for SureError {
	fn from(error: Box<dyn std::error::Error>) -> Self {
		SureError::StdError(error)
	}
}

impl From<serde_json::Error> for SureError {
	fn from(error: serde_json::Error) -> Self {
		SureError::JsonError(error)
	}
}

///
/// UreData
/// └── Vec<Marker>
///
#[derive(Debug, Serialize, Deserialize)]
struct UreData {
	markers: Vec<Marker>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Marker {
	price: String,
	id: String,
}
