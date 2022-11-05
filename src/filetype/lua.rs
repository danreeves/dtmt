use std::io::Cursor;
use std::sync::Arc;

use color_eyre::Result;
use tokio::sync::RwLock;

use crate::bundle::file::UserFile;

#[tracing::instrument(skip_all,fields(buf_len = data.as_ref().len()))]
pub(crate) async fn decompile<T>(
    _ctx: Arc<RwLock<crate::Context>>,
    data: T,
) -> Result<Vec<UserFile>>
where
    T: AsRef<[u8]>,
{
    let mut _r = Cursor::new(data.as_ref());
    todo!();
}
