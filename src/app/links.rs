use std::collections::VecDeque;

use failure::Error;
use hyper::Request;
use json::Value;
use url::form_urlencoded;

use data::{Comment, Listing};
use {App, RedditError};

impl App {
	/// Comment on a thing. The `thing` can be a post, a comment, or a private message
	/// # Arguments
	/// * `text` - The body of the comment
	/// * `thing` - Fullname of the thing to comment on
	pub fn comment(&self, text: &str, thing: &str) -> Result<(), Error> {
		let body = form_urlencoded::Serializer::new(String::new()).append_pair("text", text).append_pair("thing_id", thing).finish();

		let req = Request::post("https://oauth.reddit.com/api/comment").body(body.into()).unwrap();

		self.conn.run_auth_request(req)?;
		Ok(())
	}

	/// Edits a thing. The `thing` can be a post or a comment.
	/// # Arguments
	/// * `text` - The new body of the thing
	/// * `thing` - Fullname of the thing to edit
	pub fn edit(&self, text: &str, thing: &str) -> Result<(), Error> {
		let body = form_urlencoded::Serializer::new(String::new()).append_pair("text", text).append_pair("thing_id", thing).finish();
		let req = Request::post("https://oauth.reddit.com/api/editusertext").body(body.into()).unwrap();
		self.conn.run_auth_request(req)?;
		Ok(())
	}

	/// Load more comments from a comment tree that is not completely loaded. This function at the moment can only be called
	/// internally due to requiring `morechildren_id` that is not available in the `Thread` type.
	/// # Arguments
	/// * `link_id` - The id of the post that has the comments that are being loaded
	/// * `morechildren_id` - The id of the morechildren object that is being loaded
	/// * `comments` - Slice of `&str`s that are the ids of the comments to be loaded
	pub fn more_children(&self, link_id: &str, morechildren_id: &str, comments: &[&str]) -> Result<Listing<Comment>, Error> {
		let mut string = String::from("t3_");
		let link_id = if !link_id.starts_with("t3_") {
			string.push_str(link_id);
			&string
		} else {
			link_id
		};

		let limit = 5;
		// Break requests into chunks of `limit`
		let mut chunks: Vec<String> = Vec::new();
		let mut chunk_buf = String::new();
		for (i, id) in comments.iter().enumerate() {
			if i != 0 && i % limit == 0 {
				chunk_buf.pop(); // Removes trailing comma
				chunks.push(chunk_buf);
				chunk_buf = String::new();
			}

			chunk_buf.push_str(&format!("{},", id));
		}
		chunk_buf.pop(); // Removes trailing comma on unfinished chunk
		chunks.push(chunk_buf);

		trace!("Chunks are {:?}", chunks);

		let mut lists = Vec::new();

		for chunk in chunks {
			let body = form_urlencoded::Serializer::new(String::new())
				.append_pair("children", &chunk)
				.append_pair("link_id", link_id)
				.append_pair("id", morechildren_id)
				.append_pair("api_type", "json")
				.finish();
			trace!("Getting more children {} from {}", chunk, link_id);

			//let mut req = Request::new(Method::Get, Url::parse_with_params("https://www.reddit.com/api/morechildren/.json", params)?.into_string().parse()?);
			let req = Request::post("https://www.reddit.com/api/morechildren/.json").body(body.into()).unwrap();
			let data = self.conn.run_request(req)?;

			trace!("Scanning {}", data);

			let list: Listing<Comment> = Listing::from_value(&data["json"]["data"]["things"], link_id, self)?;
			lists.push(list);
		}

		// Flatten the vec of listings
		let mut final_list = VecDeque::new();
		for list in &mut lists {
			final_list.append(&mut list.children);
		}
		let mut listing: Listing<Comment> = Listing::new();

		for comment in final_list {
			listing.insert_comment(comment);
		}

		Ok(listing)
	}

	/// Sticky a post in a subreddit. Does nothing if the post is already stickied
	/// # Arguments
	/// * `sticky` - boolean value. True to set post as sticky, false to unset post as sticky
	/// * `slot` - Optional slot number to fill (can only be 1 or 2, and will error otherwise)
	/// * `id` - _fullname_ of the post to sticky
	pub fn set_sticky(&self, sticky: bool, slot: Option<i32>, id: &str) -> Result<(), Error> {
		let mut body = form_urlencoded::Serializer::new(String::new());
		body.append_pair("state", if sticky { "1" } else { "0" });

		if let Some(num) = slot {
			if num != 1 && num != 2 {
				return Err(Error::from(RedditError::BadRequest {
					request: "Sticky's are limited to slots 1 and 2".to_string(),
					response: "not sent".to_string(),
				}));
			}
			let numstr = num.to_string();
			body.append_pair("num", &numstr);
		}

		body.append_pair("id", id);

		let req = Request::post("https://oauth.reddit.com/api/set_subreddit_sticky/.json").body(body.finish().into()).unwrap();

		self.conn.run_auth_request(req).ok();

		Ok(())
	}

	/// Submit a self post
	/// # Arguments
	/// * `sub` - Name of the subreddit to submit a post to
	/// * `title` - Title of the post
	/// * `text` - Body of the post
	/// * `sendreplies` - Whether replies should be forwarded to the inbox of the submitter
	/// # Returns
	/// A result with reddit's json response to the submission
	pub fn submit_self(&self, sub: &str, title: &str, text: &str, sendreplies: bool) -> Result<Value, Error> {
		let body = form_urlencoded::Serializer::new(String::new())
			.append_pair("sr", sub)
			.append_pair("kind", "self")
			.append_pair("title", title)
			.append_pair("text", text)
			.append_pair("sendreplies", if sendreplies { "true" } else { "false" })
			.finish();

		let req = Request::post("https://oauth.reddit.com/api/submit/.json").body(body.into()).unwrap();

		self.conn.run_auth_request(req)
	}
}
