use rusoto_core::RusotoError;
use rusoto_s3::{CompleteMultipartUploadError, CreateMultipartUploadError, PutObjectError, UploadPartError};
use std::{
    error::Error,
    fmt::{Debug, Display, Formatter, Result as FmtResult},
    io::Error as IOError,
};

/// The error type returned by the send-file-to-S3 asynchronous jobs.
#[derive(Debug)]
pub(crate) enum SendFileError {
    CompleteMultipartUpload(RusotoError<CompleteMultipartUploadError>),
    CreateMultipartUpload(RusotoError<CreateMultipartUploadError>),
    IO(IOError),
    NoUploadPartId,
    PutObject(RusotoError<PutObjectError>),
    UploadPart(RusotoError<UploadPartError>),
}

impl From<IOError> for SendFileError {
    fn from(e: IOError) -> Self {
        Self::IO(e)
    }
}

impl From<RusotoError<CompleteMultipartUploadError>> for SendFileError {
    fn from(e: RusotoError<CompleteMultipartUploadError>) -> Self {
        Self::CompleteMultipartUpload(e)
    }
}

impl From<RusotoError<CreateMultipartUploadError>> for SendFileError {
    fn from(e: RusotoError<CreateMultipartUploadError>) -> Self {
        Self::CreateMultipartUpload(e)
    }
}

impl From<RusotoError<PutObjectError>> for SendFileError {
    fn from(e: RusotoError<PutObjectError>) -> Self {
        Self::PutObject(e)
    }
}

impl From<RusotoError<UploadPartError>> for SendFileError {
    fn from(e: RusotoError<UploadPartError>) -> Self {
        Self::UploadPart(e)
    }
}

impl Display for SendFileError {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        Debug::fmt(self, f)
    }
}

impl Error for SendFileError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::CompleteMultipartUpload(e) => Some(e),
            Self::CreateMultipartUpload(e) => Some(e),
            Self::IO(e) => Some(e),
            Self::PutObject(e) => Some(e),
            Self::UploadPart(e) => Some(e),
            _ => None,
        }
    }
}

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
