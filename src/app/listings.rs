use std::collections::HashMap;

use failure::Error;
use hyper::{Body, Request};
use json::Value;
use url::{form_urlencoded, Url};

use data::{Comment, Comments, Listing, Post, Thing};
use net::uri_params_from_map;
use {App, Sort};

macro_rules! options {
    ($(#[$($sattr:tt)*])*
     $v:vis struct $name:ident $(<$($lt:tt),*>)* {
        $($(#[$($attr:tt)*])*
          $fname:ident : $typ:ty ,)*
    }) => {
        $(#[$($sattr)*])*
        $v struct $name $(<$($lt),*>)* {
            $($(#[$($attr)*])* $fname: Option<$typ>,)*
        }
        impl $(<$($lt),*>)* $name $(<$($lt),*>)* {
            $(
                $(#[$($attr)*])*
                $v fn $fname(&mut self, $fname: $typ) -> &mut Self {
                    self.$fname = Some($fname);
                    self
                }
            )*
        }
    }
}

options!(
	/// A builder struct for constructing options for listings. The vague doc comments come
	/// from the reddit api documentation.
	#[derive(Default)]
	pub struct UserListingOpts<'a> {
		/// Context to show between 2 and 10
		context: u32,
		/// one of (given)
		show: &'a str,
		/// one of (hot, new, top, controversial)
		sort: &'a str,
		/// one of (hour, day, week, month, year, all)
		t: &'a str,
		/// one of (links, comments)
		typ: &'a str,
		/// fullname of a thing
		after: &'a str,
		/// fullname of a thing
		before: &'a str,
		/// a positive integer (default: 0)
		count: u32,
		/// boolean value
		include_categories: bool,
		/// the maximum number of items desired (default: 25, maximum: 100)
		limit: u32,
	}
);

impl UserListingOpts<'_> {
	fn append_opts<T: form_urlencoded::Target>(&self, form: &mut form_urlencoded::Serializer<T>) {
		trait Val {
			type Output: AsRef<str>;
			fn val(&self) -> Self::Output;
		}
		impl<'a> Val for &'a str {
			type Output = &'a str;
			fn val(&self) -> Self::Output {
				self
			}
		}
		impl<'a> Val for u32 {
			type Output = String;
			fn val(&self) -> Self::Output {
				self.to_string()
			}
		}
		impl<'a> Val for bool {
			type Output = String;
			fn val(&self) -> Self::Output {
				self.to_string()
			}
		}
		macro_rules! trivial {
			($s:expr, $name:ident) => {
				if let Some($name) = self.$name {
					form.append_pair($s, &Val::val(&$name));
					}
			};
		}
		trivial!("context", context);
		trivial!("show", show);
		trivial!("sort", sort);
		trivial!("t", t);
		trivial!("type", typ);
		trivial!("after", after);
		trivial!("before", before);
		trivial!("count", count);
		trivial!("include_categories", include_categories);
		trivial!("limit", limit);
	}
}

impl App {
	/// Loads a thing and casts it to the type of anything as long as it implements the Thing trait. Experimental
	/// # Arguments
	/// * `fullame` - fullname of the thing
	pub fn load_post(&self, fullname: &str) -> Result<Post, Error> {
		let mut params: HashMap<&str, &str> = HashMap::new();
		params.insert("names", fullname);

		let req = Request::get(format!("https://www.reddit.com/by_id/{}/.json", fullname)).body(Body::empty()).unwrap();
		let response = self.conn.run_request(req)?;

		Post::from_value(&response, self)
	}

	/// Get the posts in a subreddit sorted in a specific way
	/// # Arguments
	/// * `sub` - Name of subreddit to query
	/// * `sort` - Sort method of query
	/// # Returns
	/// A result containing a json listing of posts
	pub fn get_posts(&self, sub: &str, sort: Sort) -> Result<Value, Error> {
		let req = Request::get(
			Url::parse_with_params(
				&format!(
					"https://www.reddit.com/r/{}/.\
					 json",
					sub
				),
				sort.param(),
			)?
			.into_string(),
		)
		.body(Body::empty())
		.unwrap();

		self.conn.run_request(req)
	}

	/// Get a iterator of all comments in order of being posted
	/// # Arguments
	/// * `sub` - Name of the subreddit to pull comments from. Can be 'all' to pull from all of reddit
	pub fn create_comment_stream(&self, sub: &str) -> Comments {
		Comments::new(self, sub)
	}

	/// Gets the most recent comments in a subreddit. This function is also usually called internally but
	/// can be called if a one time retrieval of recent comments from a subreddit is necessary
	/// # Arguments
	/// * `sub` - Subreddit to load recent comments from
	/// * `limit` - Optional limit to amount of comments loaded
	/// * `before` - Optional comment to be the starting point for the next comments loaded
	/// # Returns
	/// A listing of comments that should be flat (no replies)
	pub fn get_recent_comments(&self, sub: &str, limit: Option<i32>, before: Option<&str>) -> Result<Listing<Comment>, Error> {
		let limit_str;
		let mut params: HashMap<&str, &str> = HashMap::new();
		if let Some(limit) = limit {
			limit_str = limit.to_string();
			params.insert("limit", &limit_str);
		}
		if let Some(ref before) = before {
			params.insert("before", before);
		}

		let req = Request::get(uri_params_from_map(&format!("https://www.reddit.com/r/{}/comments.json", sub), &params)?).body(Body::empty()).unwrap();

		let resp = self.conn.run_request(req)?;
		let comments = Listing::from_value(&resp["data"]["children"], "", self)?;

		Ok(comments)
	}

	/// Loads the comment tree of a post, returning a listing of the Comment enum, which can be
	/// either Loaded or NotLoaded
	/// # Arguments
	/// * `post` - The name of the post to retrieve the tree from
	/// # Returns
	/// A fully populated listing of commments (no `more` values)
	pub fn get_comment_tree(&self, post: &str) -> Result<Listing<Comment>, Error> {
		// TODO add sorting and shit

		let max_int = "2147483648";
		let body = form_urlencoded::Serializer::new(String::new()).append_pair("limit", max_int).append_pair("depth", max_int).finish();
		let req = Request::get(format!("https://www.reddit.com/comments/{}/.json", post)).body(body.into()).unwrap();

		let data = self.conn.run_request(req)?;
		let data = data[1]["data"]["children"].clone();

		Listing::from_value(&data, post, self)
	}

	/// Get comments made by a given user.
	pub fn get_user_comments(&self, username: &str, opts: &UserListingOpts) -> Result<Listing<Comment>, Error> {
		let mut body = form_urlencoded::Serializer::new(String::new());
		opts.append_opts(&mut body);
		let req = Request::get(format!("https://www.reddit.com/user/{}/comments.json", username)).body(body.finish().into()).unwrap();

		let data = self.conn.run_request(req)?;
		let data = data["data"]["children"].clone();

		Listing::from_value(&data, username, self)
	}
}
