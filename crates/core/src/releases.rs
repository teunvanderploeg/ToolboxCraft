use crate::{
    catalog::App,
    model::{Asset, CachedRelease, Release, Target, now},
};
use anyhow::{Context, Result, bail, ensure};
use futures_util::StreamExt;
use reqwest::{Client, StatusCode, Url};
use sha2::{Digest, Sha256};
use std::{path::Path, time::Duration};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct Releases {
    client: Client,
    api: String,
    pub(crate) github_origin: String,
}
impl Releases {
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .user_agent(concat!("ToolboxCraft/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(20))
            .timeout(Duration::from_secs(600))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 5 {
                    attempt.error("Too many redirects")
                } else if trusted_host(attempt.url()) {
                    attempt.follow()
                } else {
                    attempt.error("Download redirected outside GitHub")
                }
            }))
            .build()?;
        Ok(Self {
            client,
            api: "https://api.github.com".into(),
            github_origin: "https://github.com".into(),
        })
    }
    #[cfg(test)]
    pub(crate) fn test_server(origin: String) -> Self {
        Self {
            client: Client::new(),
            api: origin.clone(),
            github_origin: origin,
        }
    }
    pub async fn latest(&self, app: App, cache: Option<&CachedRelease>) -> Result<CachedRelease> {
        let mut request =
            self.client
                .get(format!("{}/repos/{}/releases/latest", self.api, app.repo()));
        if let Some(etag) = cache.and_then(|c| c.etag.as_ref()) {
            request = request.header("If-None-Match", etag);
        }
        let response = request
            .send()
            .await
            .context("Cannot reach GitHub. Check your connection and retry.")?;
        if response.status() == StatusCode::NOT_MODIFIED {
            let mut cached = cache
                .context("GitHub returned an empty cache response")?
                .clone();
            cached.checked_at = now();
            return Ok(cached);
        }
        if matches!(
            response.status(),
            StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS
        ) {
            let reset = response
                .headers()
                .get("x-ratelimit-reset")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("unknown");
            bail!(
                "GitHub request limit reached. Cached releases remain available. Reset time (Unix): {reset}"
            );
        }
        let response = response.error_for_status()?;
        let etag = response
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let release: Release = response.json().await?;
        ensure!(
            !release.draft && !release.prerelease,
            "Only published stable releases are supported"
        );
        semver::Version::parse(release.tag_name.trim_start_matches('v'))
            .context("Release has an unsupported version number")?;
        ensure!(
            release
                .html_url
                .starts_with(&format!("https://github.com/{}/releases/tag/", app.repo())),
            "Unexpected release repository"
        );
        Ok(CachedRelease {
            release,
            etag,
            checked_at: now(),
        })
    }
    pub async fn checksum(&self, app: App, release: &Release, asset: &Asset) -> Result<[u8; 32]> {
        let sums = release
            .assets
            .iter()
            .find(|a| a.name == "SHA256SUMS.txt")
            .context("This release has no SHA256SUMS.txt. Installation was stopped.")?;
        validate_asset_url(app, release, sums)?;
        ensure!(sums.size <= 1024 * 1024, "Checksum file is too large");
        let bytes = self.small_file(&sums.browser_download_url).await?;
        ensure!(bytes.len() <= 1024 * 1024, "Checksum file is too large");
        checksum_for(std::str::from_utf8(&bytes)?, &asset.name)
    }
    pub async fn download(
        &self,
        source: (App, &Release),
        asset: &Asset,
        path: &Path,
        expected: [u8; 32],
        cancel: &CancellationToken,
        progress: impl Fn(u64, u64),
    ) -> Result<()> {
        validate_asset_url(source.0, source.1, asset)?;
        self.download_url(
            &asset.browser_download_url,
            asset.size,
            path,
            expected,
            cancel,
            progress,
        )
        .await
    }
    pub(crate) async fn download_url(
        &self,
        url: &str,
        size: u64,
        path: &Path,
        expected: [u8; 32],
        cancel: &CancellationToken,
        progress: impl Fn(u64, u64),
    ) -> Result<()> {
        ensure!(
            size > 0 && size <= 8 * 1024 * 1024 * 1024,
            "Unexpected package size"
        );
        let result = async {
            let response = tokio::select! { _ = cancel.cancelled() => bail!("Cancelled"), r = self.client.get(url).send() => r? }.error_for_status()?;
            if let Some(length) = response.content_length() { ensure!(length == size, "Download size differs from release metadata"); }
            let mut stream = response.bytes_stream();
            let mut file = tokio::fs::File::create(path).await?;
            let mut digest = Sha256::new(); let mut received = 0;
            loop {
                let chunk = tokio::select! { _ = cancel.cancelled() => bail!("Cancelled"), chunk = stream.next() => chunk };
                let Some(chunk) = chunk else { break }; let chunk = chunk?;
                received += chunk.len() as u64; ensure!(received <= size, "Download exceeds expected size");
                digest.update(&chunk); file.write_all(&chunk).await?; progress(received, size);
            }
            ensure!(received == size, "Download was interrupted. Retry to download a complete package.");
            ensure!(<[u8; 32]>::from(digest.finalize()) == expected, "SHA-256 verification failed. This package will not be installed.");
            file.sync_all().await?; Ok(())
        }.await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(path).await;
        }
        result
    }
    pub(crate) async fn small_file(&self, url: &str) -> Result<Vec<u8>> {
        let mut stream = self
            .client
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            ensure!(
                bytes.len() + chunk.len() <= 1024 * 1024,
                "Release metadata exceeds size limit"
            );
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
}
fn trusted_host(url: &Url) -> bool {
    url.scheme() == "https"
        && matches!(
            url.host_str(),
            Some(
                "github.com"
                    | "api.github.com"
                    | "release-assets.githubusercontent.com"
                    | "objects.githubusercontent.com"
            )
        )
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none_or(|p| p == 443)
}
pub fn validate_asset_url(app: App, release: &Release, asset: &Asset) -> Result<()> {
    ensure!(
        !asset.name.contains(['/', '\\']) && !asset.name.is_empty(),
        "Invalid asset name"
    );
    let url = Url::parse(&asset.browser_download_url)?;
    let prefix = format!("/{}/releases/download/{}/", app.repo(), release.tag_name);
    ensure!(
        trusted_host(&url)
            && url.host_str() == Some("github.com")
            && url.path() == format!("{prefix}{}", asset.name)
            && url.query().is_none(),
        "Package is not from the official release repository"
    );
    Ok(())
}
pub fn select_asset(app: App, release: &Release, target: Target) -> Result<&Asset> {
    let version = release.tag_name.trim_start_matches('v');
    let expected = format!("{}-{version}-{}", app.id, target.asset_suffix());
    let mut candidates = release.assets.iter().filter(|a| a.name == expected);
    let asset = candidates
        .next()
        .context("No compatible desktop package is available for this computer")?;
    ensure!(
        candidates.next().is_none(),
        "Release contains ambiguous packages"
    );
    validate_asset_url(app, release, asset)?;
    Ok(asset)
}
pub fn checksum_for(text: &str, filename: &str) -> Result<[u8; 32]> {
    let mut found = None;
    for line in text.lines() {
        let Some((digest, name)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let name = name.trim().trim_start_matches('*');
        if name == filename {
            ensure!(found.is_none(), "Duplicate checksum entry");
            let bytes = hex::decode(digest).context("Invalid checksum")?;
            found = Some(
                bytes
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("Invalid checksum length"))?,
            );
        }
    }
    found.context("Package is missing from SHA256SUMS.txt")
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        catalog::CATALOG,
        model::{Arch, Os},
    };
    #[test]
    fn exact_package_selection_for_entire_catalog() {
        for app in CATALOG {
            for os in [Os::Mac, Os::Windows, Os::Linux] {
                for arch in [Arch::X64, Arch::Arm64] {
                    let target = Target { os, arch };
                    let name = format!("{}-1.2.3-{}", app.id, target.asset_suffix());
                    let mut r = Release {
                        tag_name: "v1.2.3".into(),
                        body: None,
                        html_url: String::new(),
                        published_at: None,
                        prerelease: false,
                        draft: false,
                        assets: vec![Asset {
                            name: name.clone(),
                            size: 4,
                            browser_download_url: format!(
                                "https://github.com/{}/releases/download/v1.2.3/{name}",
                                app.repo()
                            ),
                        }],
                    };
                    assert_eq!(select_asset(app, &r, target).unwrap().name, name);
                    r.assets[0].name = format!("{}-cli-1.2.3-{}", app.id, target.asset_suffix());
                    assert!(select_asset(app, &r, target).is_err());
                }
            }
        }
    }
    #[test]
    fn checksum_rejects_missing_duplicate_and_invalid() {
        let digest = "ab".repeat(32);
        assert_eq!(
            checksum_for(&format!("{digest} *app.zip"), "app.zip").unwrap(),
            [0xab; 32]
        );
        assert!(checksum_for(&format!("{digest} app.zip\n{digest} app.zip"), "app.zip").is_err());
        assert!(checksum_for("short app.zip", "app.zip").is_err());
        assert!(checksum_for("", "app.zip").is_err());
    }
    #[test]
    fn rejects_unofficial_redirects_and_repositories() {
        assert!(!trusted_host(
            &Url::parse("https://github.com.evil.test/file").unwrap()
        ));
        assert!(!trusted_host(
            &Url::parse("http://github.com/file").unwrap()
        ));
        assert!(!trusted_host(
            &Url::parse("https://user@github.com/file").unwrap()
        ));
    }
    fn fixture(status: &str, headers: &str, body: &str) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let response = format!("HTTP/1.1 {status}\r\nConnection: close\r\n{headers}\r\n{body}");
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            let _ = stream.read(&mut request);
            stream.write_all(response.as_bytes()).unwrap();
        });
        format!("http://{address}")
    }
    #[tokio::test]
    async fn rate_limit_and_cached_not_modified() {
        let app = CATALOG[0];
        let mut client = Releases {
            client: Client::new(),
            api: fixture("429 Too Many Requests", "Content-Length: 0\r\n", ""),
            github_origin: "https://github.com".into(),
        };
        assert!(
            client
                .latest(app, None)
                .await
                .unwrap_err()
                .to_string()
                .contains("limit")
        );
        client.api = fixture("304 Not Modified", "Content-Length: 0\r\n", "");
        let cached = CachedRelease {
            release: Release {
                tag_name: "v1.0.0".into(),
                body: None,
                html_url: String::new(),
                published_at: None,
                draft: false,
                prerelease: false,
                assets: vec![],
            },
            etag: Some("abc".into()),
            checked_at: 0,
        };
        assert!(client.latest(app, Some(&cached)).await.unwrap().checked_at > 0);
    }
    #[tokio::test]
    async fn bad_hash_incomplete_and_cancelled_downloads_leave_no_package() {
        let client = Releases {
            client: Client::new(),
            api: String::new(),
            github_origin: "https://github.com".into(),
        };
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("package");
        let url = fixture("200 OK", "Content-Length: 4\r\n", "test");
        assert!(
            client
                .download_url(
                    &url,
                    4,
                    &path,
                    [0; 32],
                    &CancellationToken::new(),
                    |_, _| {}
                )
                .await
                .is_err()
        );
        assert!(!path.exists());
        let url = fixture("200 OK", "Content-Length: 8\r\n", "test");
        assert!(
            client
                .download_url(
                    &url,
                    8,
                    &path,
                    [0; 32],
                    &CancellationToken::new(),
                    |_, _| {}
                )
                .await
                .is_err()
        );
        assert!(!path.exists());
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert!(
            client
                .download_url("http://127.0.0.1:1", 4, &path, [0; 32], &cancel, |_, _| {})
                .await
                .is_err()
        );
    }
    #[test]
    fn versions_do_not_downgrade_or_compare_lexically() {
        assert!(crate::model::newer("v0.10.0", "0.9.0"));
        assert!(!crate::model::newer("0.9.0", "0.10.0"));
        assert!(!crate::model::newer("invalid", "0.1.0"));
    }
}
