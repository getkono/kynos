//! Writing `application/json` as a response.

use kynos_openapi::model::body::mime_names;

use crate::{
    error::problem::Problem,
    http::{HeaderValue, Response, StatusCode, body::Body, header},
    response::{IntoResponse, Responses},
    schema::{Schema, registry::Registry},
};

use crate::extract::body::json::Json;

impl<T: serde::Serialize> IntoResponse for Json<T> {
    fn into_response(self) -> Response {
        // Serialized in full first, so a failure can still change the status.
        let mut writer = Writer::new();
        if serde_json::to_writer(&mut writer, &self.0).is_err() {
            return Problem::new(StatusCode::INTERNAL_SERVER_ERROR)
                .with_detail("the response body could not be serialized")
                .into_response();
        }

        let mut response = Response::new(Body::from_bytes(writer.into_bytes()));
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(mime_names::APPLICATION_JSON),
        );
        response
    }
}

/// How many octets a body is written on the stack before any heap is touched.
///
/// A body that fits is copied once into an allocation of exactly its length,
/// which `Bytes` takes without the shared header an under-filled `Vec` costs:
/// one block where `serde_json::to_vec` spent a 128-octet start, a reallocation
/// per doubling and that header. Sized past kynos-bench's `echo-post` document.
const STACK_OCTETS: usize = 1024;

/// Where a body is serialized: on the stack until it outgrows it, then on the
/// heap, starting at twice the stack so a spilled body skips the small
/// doublings.
///
/// No hint carried between responses: the first response costs what the
/// thousandth does, which is the property the counting harness holds.
struct Writer {
    stack: [u8; STACK_OCTETS],
    len: usize,
    spilled: Vec<u8>,
}

impl Writer {
    fn new() -> Self {
        Self {
            stack: [0; STACK_OCTETS],
            len: 0,
            spilled: Vec::new(),
        }
    }

    fn into_bytes(self) -> bytes::Bytes {
        if self.spilled.is_empty() {
            bytes::Bytes::copy_from_slice(&self.stack[..self.len])
        } else {
            bytes::Bytes::from(self.spilled)
        }
    }
}

impl std::io::Write for Writer {
    fn write(&mut self, octets: &[u8]) -> std::io::Result<usize> {
        self.write_all(octets)?;
        Ok(octets.len())
    }

    fn write_all(&mut self, octets: &[u8]) -> std::io::Result<()> {
        if self.spilled.is_empty() {
            let end = self.len + octets.len();
            if let Some(free) = self.stack.get_mut(self.len..end) {
                free.copy_from_slice(octets);
                self.len = end;
                return Ok(());
            }
            self.spilled = Vec::with_capacity(end.max(2 * STACK_OCTETS));
            self.spilled.extend_from_slice(&self.stack[..self.len]);
        }
        self.spilled.extend_from_slice(octets);
        Ok(())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<T: Schema> Responses for Json<T> {
    fn responses(registry: &mut Registry) -> kynos_openapi::Responses {
        kynos_openapi::Responses::new().with(
            200,
            kynos_openapi::Response::with_content(
                "OK",
                mime_names::APPLICATION_JSON,
                kynos_openapi::MediaType::new(registry.resolve::<T>()),
            ),
        )
    }
}
