use std::{
    fs::File,
    io::Write,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use hyper::HeaderMap;
use tokio::task::JoinHandle;

use anyhow::{Context, Result};

use crate::requests::{self, AuthInfo, GROUP_ONE_AUTH, GROUP_TWO_AUTH};

pub fn spawn_refresher() -> JoinHandle<Result<()>> {
    tokio::spawn(async {
        println!("Refresh task started.");

        let mut interval = tokio::time::interval(Duration::from_secs(840));

        loop {
            interval.tick().await;

            async fn refresh(auth_info: &mut AuthInfo, file_name: &str) -> Result<()> {
                println!("Refreshing cookie...");

                let mut headers = HeaderMap::new();

                headers.append("X-V-AppGuid", "2921bc596ec7b32f42a75a1e117ce40a".parse()?);
                headers.append("X-V-AppVersion", "22.08.0007.53443".parse()?);
                headers.append("X-V-RequestVerificationToken", "3GqwR36JjwPdGzir5dXSvAAJ14u7VpOenTcGKGak9JhAAyZQwiRswRISo_BbA5PDwHlLdHiqlt7BZ_6AB7KdygalChDHJhH7MkL_Bd4XnjM3kEm20".parse()?);

                let resp = requests::get(
                    format!(
                        "/{}/{}/Home.mvc/RefreshSession?_dc={}",
                        std::env::var("SYMBOL").unwrap(),
                        std::env::var("STUDENT_ID").unwrap(),
                        SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis()
                    ),
                    &auth_info,
                    requests::Host::UonetPlusUczen,
                    Some(headers),
                )
                .await?;

                let set_cookie = resp
                    .headers()
                    .get("set-cookie")
                    .context("Set-Cookie not received")?;

                for res_cookie in cookie::Cookie::split_parse(set_cookie.to_str()?) {
                    if let Ok(res_cookie) = res_cookie {
                        if res_cookie.name() == "EfebSsoCookie" {
                            if res_cookie.value() == "null" {
                                panic!("Failed to refresh cookie.");
                            }

                            auth_info.cookie = res_cookie.value().to_owned();
                            if let Ok(mut file) = File::create(file_name) {
                                if let Err(err) = file.write_all(res_cookie.value().as_bytes()) {
                                    eprintln!("Failed to write to cookie file: {err:#?}");
                                };
                            }

                            println!("Refreshed cookie: {}", auth_info.cookie);
                            break;
                        }
                    }
                }

                Ok(())
            }

            {
                let mut auth = GROUP_ONE_AUTH.lock().await;
                refresh(&mut auth, "/etc/uonetplan/cookie_1").await.unwrap();
            }
            {
                let mut auth = GROUP_TWO_AUTH.lock().await;
                refresh(&mut auth, "/etc/uonetplan/cookie_2").await.unwrap();
            }
        }
    })
}
