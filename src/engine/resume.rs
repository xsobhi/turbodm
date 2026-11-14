//! Before going on with a half-downloaded file: is the server still sending the same one? The size
//! and validators (ETag, Last-Modified) say so when the server gives them; many don't, or give
//! ones that change between their machines. So also, as a last look, the few bytes just before
//! where the download goes on are asked for again and compared with those on the disk: a
//! different file nearly always differs there, and it costs one tiny request.

use super::http::{self, Headers};
use super::segments::Segment;
use futures_util::StreamExt;
use std::path::Path;
use std::time::Duration;

/// How many bytes are compared.
const OVERLAP: u64 = 64;

/// False only when the bytes the server sends now differ from those on the disk. (Can't ask, or
/// can't read: true, as it was before this look; the other checks still stand.)
pub async fn same_bytes(client: &reqwest::Client, url: &str, headers: &Headers, part: &Path, segments: &[Segment]) -> bool {
    // the furthest a part got (more than the overlap): any would do, but the furthest in has had
    // the most time to be overwritten by something else
    let Some(end) = segments.iter().filter(|s| s.done >= OVERLAP).map(Segment::pos).max() else { return true };
    let start = end - OVERLAP;
    let Ok(Some(on_disk)) = read_at(part, start) else { return true };
    let asked = async {
        let resp = http::open(client, url, headers, Some((start, Some(end)))).await.ok()?;
        let mut body = resp.bytes_stream();
        let mut got = Vec::with_capacity(OVERLAP as usize);
        while let Some(Ok(chunk)) = body.next().await {
            got.extend_from_slice(&chunk);
            if got.len() as u64 >= OVERLAP {
                break;
            }
        }
        (got.len() as u64 >= OVERLAP).then(|| got[..OVERLAP as usize].to_vec())
    };
    match tokio::time::timeout(Duration::from_secs(20), asked).await {
        Ok(Some(sent)) => sent == on_disk,
        _ => true,
    }
}

fn read_at(part: &Path, offset: u64) -> std::io::Result<Option<Vec<u8>>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(part)?;
    file.seek(SeekFrom::Start(offset))?;
    let mut buf = vec![0u8; OVERLAP as usize];
    match file.read_exact(&mut buf) {
        Ok(()) => Ok(Some(buf)),
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(None),
        Err(e) => Err(e),
    }
}
