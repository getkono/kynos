//! Where a matched route found each of its variables, recorded by the router
//! and read back by [`Path`](super::Path) and
//! [`captured`](crate::unchecked::captured).

/// Where in the request path a matched route found each of its variables.
///
/// One span into the request path per variable the template declares, in its
/// order, so a match copies no value. A route of up to [`INLINE`] variables on
/// a path of at most `u16::MAX` octets holds its spans in place and allocates
/// nothing; any other allocates once, for all of them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PathCaptures {
    names: &'static [&'static str],
    spans: Spans,
}

/// Each variable's `[start, end)` octet offsets into the request path, narrow
/// so that a record stays small enough to move without a copy loop.
///
/// A variable the match did not capture holds a start past its end, which no
/// path slices.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Spans {
    Short([[u16; 2]; INLINE]),
    Long(Box<[[u32; 2]]>),
}

impl PathCaptures {
    /// Records what a match captured out of `path` for each of `names`, which
    /// `captured` looks up.
    ///
    /// # Panics
    ///
    /// Panics if a value is not a subslice of `path`.
    pub(crate) fn new<'a>(
        path: &str,
        names: &'static [&'static str],
        mut captured: impl FnMut(&str) -> Option<&'a str>,
    ) -> Self {
        let base = path.as_ptr() as usize;
        let mut span = |name: &str| {
            let value = captured(name)?;
            let start = value.as_ptr() as usize;
            assert!(
                start >= base && start + value.len() <= base + path.len(),
                "a path capture must borrow the path it was matched against"
            );
            let start = start - base;
            Some([start, start + value.len()])
        };

        let spans = if names.len() <= INLINE && u16::try_from(path.len()).is_ok() {
            let mut spans = [[u16::MAX, 0]; INLINE];
            for (slot, name) in spans.iter_mut().zip(names) {
                if let Some(narrowed) = span(name).and_then(narrow) {
                    *slot = narrowed;
                }
            }
            Spans::Short(spans)
        } else {
            // A path past `u32::MAX` octets captures nothing readable.
            Spans::Long(
                names
                    .iter()
                    .map(|name| span(name).and_then(narrow).unwrap_or([u32::MAX, 0]))
                    .collect(),
            )
        };
        Self { names, spans }
    }

    /// The value captured for `name`, borrowed back out of `path`.
    ///
    /// `None` when the span does not fit, as for a path rewritten after
    /// matching.
    pub(crate) fn get<'p>(&self, path: &'p str, name: &str) -> Option<&'p str> {
        let index = self.names.iter().position(|captured| *captured == name)?;
        let [start, end] = match &self.spans {
            Spans::Short(spans) => spans.get(index)?.map(usize::from),
            Spans::Long(spans) => narrow(*spans.get(index)?)?,
        };
        path.get(start..end)
    }

    /// The name of the last variable the template declares, which is the
    /// operation's own last: a prefix's variables come first.
    pub(crate) fn last(&self) -> Option<&'static str> {
        self.names.last().copied()
    }
}

/// `span` in another integer width, or `None` where either end does not fit.
fn narrow<F, N: TryFrom<F>>([start, end]: [F; 2]) -> Option<[N; 2]> {
    Some([N::try_from(start).ok()?, N::try_from(end).ok()?])
}

/// How many variables a match records in place, and a group decodes on the
/// stack, before either takes the heap.
///
/// One past `matchit`'s own inline capacity of three, so the shortest route
/// that spills here already paid one allocation for its variables there.
pub(crate) const INLINE: usize = 4;

#[cfg(test)]
pub(super) mod tests;
