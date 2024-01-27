use anyhow::{Context, Result};
use chrono::NaiveDate;
use hyper::HeaderMap;
use serde::Deserialize;
use serde_json::json;

use crate::requests::{self, AuthInfo};

#[derive(Deserialize, Debug)]
pub struct WeekPlanResponse {
    pub success: bool,
    pub data: WeekPlanData,
}

#[derive(Deserialize, Debug)]
pub struct WeekPlanData {
    #[serde(rename = "Data")]
    pub date: String,
    #[serde(rename = "Headers")]
    pub headers: Vec<WeekPlanHeader>,
    #[serde(rename = "Rows")]
    pub rows: Vec<Vec<String>>,
}

#[derive(Deserialize, Debug)]
pub struct WeekPlanHeader {
    #[serde(rename = "Text")]
    pub text: String,
}

pub async fn get_week_plan(day: NaiveDate, auth_info: &AuthInfo) -> Result<WeekPlanResponse> {
    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", "application/json".try_into()?);

    let res = requests::post(
        format!("/{}/{}/PlanZajec.mvc/Get", std::env::var("SYMBOL").unwrap(), std::env::var("STUDENT_ID").unwrap()),
        auth_info,
        requests::Host::UonetPlusUczen,
        Some(
            json!({ "data": format!("{}T00:00:00", day.format("%Y-%m-%d").to_string()) })
                .to_string(),
        ),
        Some(headers),
    )
    .await?;

    let body = requests::body_text(res.into_body()).await?;

    serde_json::from_str::<WeekPlanResponse>(&body).context("Failed to parse response data.")
}
