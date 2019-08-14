use std::collections::VecDeque;

use data::Comment;
use App;

/// A struct that represents a stream of comments from a subreddit as they are posted. To use it
/// simply create a `for` loop with this is the source. It will automatically retrieve comments
/// as needed. The subreddit can be `all` to create a stream of comments from all of reddit.
pub struct Comments<'a> {
	sub: String,
	cache: VecDeque<Comment>,
	last: Option<String>,
	app: &'a App,
}

impl<'a> Comments<'a> {
	/// Creates a stream of comments from a subreddit
	/// # Arguments
	/// * `app` - A reference to a Reddit `App` instance
	/// * `sub` - The subreddit to load comments from. Can be "all" to stream comments from all
	/// of reddit.
	pub fn new(app: &'a App, sub: &str) -> Comments<'a> {
		let cache: VecDeque<Comment> = VecDeque::new();
		let last = None;

		Comments { sub: sub.to_string(), cache, last, app }
	}

	fn refresh(&mut self, app: &App) {
		let mut fails = 0;
		let resp = loop {
			match app.get_recent_comments(&self.sub, Some(500), self.last.as_ref().map(|s| s.as_str())) {
				Ok(x) => break x,
				Err(e) => {
					log::warn!("Error from get_recent_comments, retrying: {}\n", e);
					std::thread::sleep(std::time::Duration::from_millis(fails * 1000 + rand::random::<u64>() % 1000));
					fails = (fails + 1).min(10);
					continue;
				}
			}
		};

		if let Some(comment) = resp.children.front() {
			self.last = Some(comment.name.clone());
		}

		// The result is in reverse-chronological order, so put the reverse order in the
		// cache.
		self.cache.extend(resp.children.into_iter().rev());
	}
}

impl<'a> Iterator for Comments<'a> {
	type Item = Comment;

	fn next(&mut self) -> Option<Self::Item> {
		while self.cache.is_empty() {
			self.refresh(self.app);
		}
		self.cache.pop_front()
	}
}

/// Sort type of a subreddit
pub enum Sort {
	/// Hot
	Hot,
	/// New
	New,
	/// Rising
	Rising,
	/// Top within the specified `SortTime`
	Top(SortTime),
	/// Most controversial within the specified `SortTime`
	Controversial(SortTime),
}

impl Sort {
	/// Convert to url parameters
	pub fn param<'a>(self) -> Vec<(&'a str, &'a str)> {
		use self::Sort::*;
		match self {
			Hot => vec![("sort", "hot")],
			New => vec![("sort", "new")],
			Rising => vec![("sort", "rising")],
			Top(sort) => vec![("sort", "top"), sort.param()],
			Controversial(sort) => vec![("sort", "controversial"), sort.param()],
		}
	}
}

/// Time parameter of a subreddit sort
pub enum SortTime {
	/// Hour
	Hour,
	/// Day
	Day,
	/// Week
	Week,
	/// Month
	Month,
	/// Year
	Year,
	/// All time
	All,
}

impl SortTime {
	/// Convert the sort time to a tuple to be used in url parameters
	pub fn param<'a>(self) -> (&'a str, &'a str) {
		use self::SortTime::*;
		(
			"t",
			match self {
				Hour => "hour",
				Day => "day",
				Week => "week",
				Month => "month",
				Year => "year",
				All => "all",
			},
		)
	}
}
