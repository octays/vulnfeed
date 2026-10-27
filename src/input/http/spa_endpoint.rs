use std::{marker::PhantomData, path::PathBuf};

use poem::{
    Endpoint, Request, Response,
    endpoint::EmbeddedFileEndpoint,
    http::{Method, StatusCode},
};
use rust_embed::RustEmbed;

/// An endpoint that serves static files for a Single Page Application (SPA).
///
/// This endpoint will serve static files normally, but when a file is not found,
/// it will serve the index.html file instead, allowing client-side routing to take over.
pub struct SpaFileEndpoint<E: RustEmbed + Send + Sync> {
    _embed: PhantomData<E>,
}

impl<E: RustEmbed + Send + Sync> SpaFileEndpoint<E> {
    pub fn new() -> Self {
        SpaFileEndpoint {
            _embed: PhantomData,
        }
    }
}

impl<E: RustEmbed + Send + Sync> Default for SpaFileEndpoint<E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: RustEmbed + Send + Sync> Endpoint for SpaFileEndpoint<E> {
    type Output = Response;

    async fn call(&self, req: Request) -> Result<Self::Output, poem::Error> {
        if req.method() != Method::GET {
            return Err(StatusCode::METHOD_NOT_ALLOWED.into());
        }

        let path = req.uri().path().trim_start_matches('/');

        if E::get(path).is_some() {
            return EmbeddedFileEndpoint::<E>::new(path).call(req).await;
        }
        EmbeddedFileEndpoint::<E>::new("index.html").call(req).await
    }
}

/// An endpoint that serves SPA static files from a directory on disk.
///
/// Files are served from the directory; when a path is not found it falls
/// back to index.html so client-side routing behaves like the embedded one.
pub struct SpaDirEndpoint {
    root: PathBuf,
}

impl SpaDirEndpoint {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        SpaDirEndpoint { root: root.into() }
    }

    /// 读取 root 内的文件;canonicalize 双侧比对防止目录穿越
    async fn read_under_root(&self, rel: &str) -> Option<Vec<u8>> {
        let root = tokio::fs::canonicalize(&self.root).await.ok()?;
        let canonical = tokio::fs::canonicalize(root.join(rel)).await.ok()?;
        if !canonical.starts_with(&root) {
            return None;
        }
        tokio::fs::read(canonical).await.ok()
    }
}

fn static_mime(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or_default() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "application/javascript",
        "css" => "text/css",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

impl Endpoint for SpaDirEndpoint {
    type Output = Response;
    async fn call(&self, req: Request) -> Result<Self::Output, poem::Error> {
        if req.method() != Method::GET {
            return Err(StatusCode::METHOD_NOT_ALLOWED.into());
        }
        let path = req.uri().path().trim_start_matches('/');
        if !path.is_empty()
            && !path.contains("..")
            && let Some(bytes) = self.read_under_root(path).await
        {
            return Ok(Response::builder()
                .content_type(static_mime(path))
                .body(bytes));
        }
        // SPA 回退:未命中路径返回目录下的 index.html
        match self.read_under_root("index.html").await {
            Some(bytes) => Ok(Response::builder()
                .content_type("text/html; charset=utf-8")
                .body(bytes)),
            None => Err(StatusCode::NOT_FOUND.into()),
        }
    }
}
