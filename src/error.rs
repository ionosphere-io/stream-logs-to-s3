use std::{
    error::Error,
    fmt::{Debug, Display, Formatter, Result as FmtResult},
};

/// An error type for why we rejected a user's S3 URL.
#[derive(Debug, PartialEq)]
pub(crate) enum InvalidS3URL {
    InvalidURLFormat(String, String),
    InvalidTemplateSyntax(String),
}

impl Display for InvalidS3URL {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        match self {
            Self::InvalidURLFormat(reason, url) => {
                write!(f, "Invalid S3 URL format: {}: {}", reason, url)
            }
            Self::InvalidTemplateSyntax(msg) => {
                write!(f, "Invalid template syntax: {}", msg)
            }
        }
    }
}

impl Error for InvalidS3URL {}

/// Error type for non-Unix platforms representing a bad file type
#[cfg(not(unix))]
#[derive(Debug)]
pub(crate) struct BadFileTypeError {}

#[cfg(not(unix))]
impl Display for BadFileTypeError {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        write!(f, "Bad file type")
    }
}

#[cfg(not(unix))]
impl Error for BadFileTypeError {}
