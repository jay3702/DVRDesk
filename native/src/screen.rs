//! Replaces React Router's route table. No URL bar exists natively, so the
//! old app's query-param deep-linking (TVShows's showId/episodeId/filter,
//! Movies's movieId, Library's groupId/videoId) becomes fields directly on
//! the enum variant instead of a serialized query string.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    Recent,
    Live,
    TvShows {
        show_id: Option<String>,
        episode_id: Option<String>,
        filter: Option<String>,
    },
    Movies {
        movie_id: Option<String>,
    },
    Library {
        group_id: Option<String>,
        video_id: Option<String>,
    },
    Collections {
        collection_id: Option<String>,
        show_id: Option<String>,
    },
    Search,
    Downloads,
    Settings,
}

impl Default for Screen {
    fn default() -> Self {
        Screen::Recent
    }
}

impl Screen {
    pub fn tv_shows() -> Self {
        Screen::TvShows {
            show_id: None,
            episode_id: None,
            filter: None,
        }
    }

    pub fn movies() -> Self {
        Screen::Movies { movie_id: None }
    }

    pub fn library() -> Self {
        Screen::Library {
            group_id: None,
            video_id: None,
        }
    }

    pub fn collections() -> Self {
        Screen::Collections {
            collection_id: None,
            show_id: None,
        }
    }

    /// Label shown in the sidebar and used for the active-tab highlight —
    /// matches variant identity, not the carried deep-link fields, the same
    /// way NavLink's `end` matching in the old sidebar ignored query params.
    pub fn nav_label(&self) -> &'static str {
        match self {
            Screen::Recent => "Recent",
            Screen::Live => "Live",
            Screen::TvShows { .. } => "TV Shows",
            Screen::Movies { .. } => "Movies",
            Screen::Library { .. } => "Videos",
            Screen::Collections { .. } => "Collections",
            Screen::Search => "Search",
            Screen::Downloads => "Downloads",
            Screen::Settings => "Settings",
        }
    }
}
