//! The module contains networking, http, ratelimiting, authorization and more functionality.
//!
//! Most use cases of this library will not require anything directly present in this module
//! explicitly, but be sure to read the documentation in the auth module for any script that wants
//! to authorize itself on reddit.

/// Contains all functionality for OAuth and logins
pub mod auth;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::hash::BuildHasher;
use std::thread;
use std::time::{Duration, Instant};

use futures::Stream;
use hyper::client::{Client, HttpConnector};
use hyper::header::{self, HeaderValue};
use hyper::{Body, Request, Response, Uri};
use hyper_tls::HttpsConnector;
use json;
use json::Value;
use tokio_core::reactor::Core;

use self::auth::OAuth;
use errors::RedditError;

use failure::Error;

/// How to ratelimit
#[derive(Copy, Clone)]
pub enum LimitMethod {
	/// Wait an even amount of time between each request
	Steady,
	/// Fire off requests as they come. It's possible there will be a long waiting time for the
	/// next ratelimit period if too many are fired off at once.
	Burst,
}

/// A connection holder to reddit. Holds authorization info if provided, and is in charge
/// of ratelimiting.
pub struct Connection {
	/// Authorization info (optional, but required for sending authorized requests)
	pub auth: Option<auth::OAuth>,
	/// User agent for the client
	pub useragent: HeaderValue,
	/// HTTP client
	pub client: Client<HttpsConnector<HttpConnector>, Body>,
	/// Tokio core
	core: RefCell<Core>,
	/// How to ratelimit (burst or steady)
	pub limit: Cell<LimitMethod>,
	/// Requests sent in the past ratelimit period
	reqs: Cell<i32>,
	/// Requests remaining
	remaining: Cell<Option<i32>>,
	/// Time when request amount will reset
	reset_time: Cell<Instant>,
}

impl Connection {
	/// Creates a new connection instance to reddit
	/// # Arguments
	/// * `appname` - The name of the app
	/// * `appversion` - The version of the app
	/// * `appauthor` - The author of the app (should be in reddit form as /u/<username>)
	pub fn new(appname: &str, appversion: &str, appauthor: &str) -> Result<Connection, Error> {
		let useragent = HeaderValue::from_str(&format!("linux:{}:{} (by {})", appname, appversion, appauthor)).unwrap();
		let core = Core::new()?;
		let client = Client::builder().build(HttpsConnector::new(1)?);
		Ok(Connection {
			auth: None,
			useragent,
			client,
			core: RefCell::new(core),
			limit: Cell::new(LimitMethod::Steady),
			reqs: Cell::new(0),
			remaining: Cell::new(None),
			reset_time: Cell::new(Instant::now()),
		})
	}

	/// Send a request to reddit. This is where ratelimiting happens, as well as setting the
	/// user agent.
	pub fn run_request(&self, mut req: Request<Body>) -> Result<Value, Error> {
		let req_str = format!("{:?}", req);

		// Ratelimit based on method chosen type
		match self.limit.get() {
			LimitMethod::Steady => {
				// Check if we have a remaining limit
				if let Some(remaining) = self.remaining.get() {
					// If the reset time is in the future
					if Instant::now() < self.reset_time.get() {
						trace!("Ratelimiting in steady mode for {:?}", self.reset_time.get() - Instant::now());
						// Sleep for the amount of time until reset divided by how many requests we have for steady sending
						thread::sleep((self.reset_time.get() - Instant::now()).checked_div(remaining as u32).unwrap());
					}
					// Else we must have already passed reset time and we will get a new one after this request
				}
			}
			LimitMethod::Burst => {
				// Check if we have a remaining limit
				if let Some(remaining) = self.remaining.get() {
					// If we have none remaining and we haven't passed the request limit, sleep till we do
					if remaining <= 0 && self.reset_time.get() > Instant::now() {
						trace!("Ratelimiting in burst mode for {:?}", self.reset_time.get() - Instant::now());
						thread::sleep(self.reset_time.get() - Instant::now());
					}
				}
			}
		};

		// Set useragent
		req.headers_mut().insert(header::USER_AGENT, self.useragent.clone());

		// Log the request
		trace!("Sending request {:?}", req);

		// Execute the request!
		let response = self.client.request(req);
		let response = self.core.borrow_mut().run(response)?;

		// Update values from response ratelimiting headers
		if let Some(reqs_used) = response.headers().get("x-ratelimit-used") {
			let reqs_used = reqs_used.to_str().unwrap().parse::<f32>().unwrap().round() as i32;
			trace!("Used {} of requests in ratelimit period", reqs_used);
			self.reqs.set(reqs_used);
		}
		if let Some(reqs_remaining) = response.headers().get("x-ratelimit-remaining") {
			let reqs_remaining = reqs_remaining.to_str().unwrap().parse::<f32>().unwrap().round() as i32;
			trace!("Have {} requests remaining in ratelimit period", reqs_remaining);
			self.remaining.set(Some(reqs_remaining));
		}
		if let Some(secs_remaining) = response.headers().get("x-ratelimit-reset") {
			let secs_remaining = secs_remaining.to_str().unwrap().parse::<f32>().unwrap().round() as u64;
			trace!("Have {} seconds remaining to ratelimit reset", secs_remaining);
			self.reset_time.set(Instant::now() + Duration::new(secs_remaining, 0));
		}
		trace!("Ratelimiting:\n\tRequests used: {:?}\n\tRequests remaining: {:?}\n\tReset time: {:?}\n\tNow: {:?}", self.reqs.get(), self.remaining.get(), self.reset_time.get(), Instant::now());

		let response_str = format!("{:?}", response);
		let get_body = |response: Response<Body>| -> Result<String, Error> {
			let body = self.core.borrow_mut().run(response.into_body().concat2())?;
			let body: String = String::from_utf8_lossy(&body).into();
			Ok(body)
		};

		if !response.status().is_success() {
			error!("Got error response: {}", response_str);
			return Err(Error::from(RedditError::BadRequest {
				request: req_str,
				response: format!("Reponse: {}\nResponse body: {:?}", response_str, get_body(response)?),
			}));
		}

		let body = get_body(response)?;

		match json::from_str(&body) {
			Ok(r) => {
				trace!("Got successful response: {:?}\nBody: {}", response_str, body);
				Ok(r)
			}
			Err(_) => Err(Error::from(RedditError::BadResponse { request: req_str, response: body })),
		}
	}

	/// Send a request to reddit with authorization headers
	pub fn run_auth_request(&self, mut req: Request<Body>) -> Result<Value, Error> {
		if let Some(ref auth) = self.auth {
			let token = match auth {
				OAuth::Script {
					id: ref _id,
					secret: ref _secret,
					username: ref _username,
					password: ref _password,
					ref token,
					ref expire_instant,
				} => {
					use std::ops::Add;
					if Instant::now().add(Duration::from_secs(120)) > expire_instant.get() {
						// About to expire. Renew now.
						auth.refresh(self)?;
					}
					token.borrow()
				}
				OAuth::InstalledApp {
					id: ref _id,
					redirect: ref _redirect,
					ref token,
					ref refresh_token,
					ref expire_instant,
				} => {
					// If the token can expire and we are able to refresh it
					if let (Some(_refresh_token), Some(expire_instant)) = (refresh_token.borrow().clone(), expire_instant.get()) {
						// If the token's expired, refresh it
						if Instant::now() > expire_instant {
							auth.refresh(self)?;
						}
						token.borrow()
					} else if let Some(expire_instant) = expire_instant.get() {
						if Instant::now() > expire_instant {
							return Err(Error::from(RedditError::Forbidden { request: format!("{:?}", req) }));
						} else {
							token.borrow()
						}
					} else {
						token.borrow()
					}
				}
			};
			let token_str: &str = &*token;
			req.headers_mut().insert(header::AUTHORIZATION, HeaderValue::from_str(&format!("Bearer {}", token_str)).unwrap());
			self.run_request(req)
		} else {
			Err(Error::from(RedditError::Forbidden { request: format!("{:?}", req) }))
		}
	}

	/// Set's the ratelimiting method
	pub fn set_limit(&self, limit: LimitMethod) {
		self.limit.set(limit);
	}

	/// Returns a reference to the tokio core in a RefCell
	pub fn get_core(&self) -> &RefCell<Core> {
		&self.core
	}
}

/// Creates a url with encoded parameters from hashmap. Right now it's kinda hacky
pub fn uri_params_from_map<S: BuildHasher>(url: &str, map: &HashMap<&str, &str, S>) -> Result<Uri, Error> {
	use url::Url;

	Ok(Url::parse_with_params(url, map)?.to_string().parse()?)
}
