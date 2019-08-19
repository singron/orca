mod account;
mod auth;
mod links;
mod listings;
mod messages;
mod users;

use failure::Error;

use net::{Connection, LimitMethod};

pub use self::listings::UserListingOpts;

/// A reddit object
/// ## Usage:
/// To create a new instance, use `Reddit::new()`
pub struct App {
	pub(crate) conn: Connection,
}

impl App {
	/// Create a new reddit instance
	/// # Arguments
	/// * `appname` - Unique app name
	/// * `appversion` - App version
	/// * `appauthor` - Auther of the app
	/// # Returns
	/// A new reddit object
	pub fn new(appname: &str, appversion: &str, appauthor: &str) -> Result<App, Error> {
		Ok(App { conn: Connection::new(appname, appversion, appauthor)? })
	}

	/// Sets the method to use for ratelimiting.
	/// # Arguments
	/// * `limit` - The method to use for ratelimiting
	pub fn set_ratelimiting(&self, limit: LimitMethod) {
		self.conn.set_limit(limit);
	}
}
