//! A `multipart/form-data` request body assembled part by part.
//!
//! [`TestRequest::multipart`](crate::test::TestRequest::multipart) frames it,
//! with the same writer a [`MultipartForm`] response uses, under a delimiter no
//! part contains; a type deriving `MultipartForm` is sent the same way.
//!
//! [`MultipartForm`]: crate::extract::body::multipart::MultipartForm

use bytes::Bytes;

use crate::{extract::body::multipart::Part, response::codec::multipart::IntoMultipart};

/// The parts of a `multipart/form-data` body, written in the order they are
/// added, as a browser submitting a form writes them.
///
/// ```
/// # async fn run(client: kynos::test::TestClient<()>) {
/// use kynos::test::multipart::TestMultipart;
///
/// client
///     .post("/imports")
///     .multipart(
///         TestMultipart::new()
///             .text("source", "mal_xml")
///             .file("file", "list.xml", "application/xml", "<list/>"),
///     )
///     .send()
///     .await
///     .assert_status(201);
/// # }
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TestMultipart {
    parts: Vec<Part>,
}

impl TestMultipart {
    /// A body with no parts yet.
    #[must_use]
    pub const fn new() -> Self {
        Self { parts: Vec::new() }
    }

    /// Adds a form field carrying `value`.
    ///
    /// The part states no `Content-Type`, as a browser's text field does, so
    /// it is `text/plain` by RFC 7578's default.
    #[must_use]
    pub fn text(self, name: &str, value: &str) -> Self {
        self.part(Part {
            name: name.to_owned(),
            file_name: None,
            content_type: None,
            bytes: Bytes::from(value.to_owned()),
        })
    }

    /// Adds an uploaded file: its octets, the name the client gave it, and
    /// the media type it is in.
    #[must_use]
    pub fn file(
        self,
        name: &str,
        file_name: &str,
        content_type: &str,
        bytes: impl Into<Bytes>,
    ) -> Self {
        self.part(Part {
            name: name.to_owned(),
            file_name: Some(file_name.to_owned()),
            content_type: Some(content_type.to_owned()),
            bytes: bytes.into(),
        })
    }

    /// Adds a part as given, for what [`text`](Self::text) and
    /// [`file`](Self::file) do not write: a part with a media type and no
    /// file name, or a file with no media type.
    #[must_use]
    pub fn part(mut self, part: Part) -> Self {
        self.parts.push(part);
        self
    }
}

impl IntoMultipart for TestMultipart {
    fn into_parts(self) -> Vec<Part> {
        self.parts
    }
}
