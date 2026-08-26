//! README anti-pattern 4: a `Problem` carries its status in a field, so
//! returning one would choose that status at run time.
//!
//! It implements `IntoResponse` — being written is what it is for — and not
//! `Responses`, because it cannot say which statuses it produces. The handler
//! bound is the pair, so the missing half is what rejects it.

fn returns<T: kynos::response::IntoResponse + kynos::response::Responses>() {}

fn main() {
    returns::<Result<kynos::response::status::NoContent, kynos::Problem>>();
}
