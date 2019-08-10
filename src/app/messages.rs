use failure::Error;
use hyper::Request;
use url::form_urlencoded;

use App;

impl App {
	/// Send a private message to a user
	/// # Arguments
	/// * `to` - Name of the user to send a message to
	/// * `subject` - Subject of the message
	/// * `body` - Body of the message
	pub fn message(&self, to: &str, subject: &str, body: &str) -> Result<(), Error> {
		let form = form_urlencoded::Serializer::new(String::new()).append_pair("to", to).append_pair("subject", subject).append_pair("text", body).finish();

		let req = Request::post("https://oauth.reddit.com/api/compose/.json").body(form.into()).unwrap();

		match self.conn.run_auth_request(req) {
			Ok(_) => Ok(()),
			Err(e) => Err(e),
		}
	}
}
