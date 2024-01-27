mod cookie_refresher;
mod endpoints;
mod requests;

use actix_web::{get, App, HttpServer, Responder};
use anyhow::{bail, Context, Result};
use chrono::Datelike;
use chrono::NaiveDate;
use chrono::Weekday;
use chrono::{Local, Timelike, Utc};
use html_parser::{Dom, Node};

use hyper::Body;
use ics::{components::Property, Event, ICalendar};
use requests::body_text;
use requests::AuthInfo;
use requests::Group;
use requests::GROUP_ONE_AUTH;
use requests::GROUP_ONE_CACHE;
use requests::GROUP_TWO_AUTH;
use requests::GROUP_TWO_CACHE;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs};

#[derive(Deserialize, Debug)]
struct LessonPlanResponse {
    data: Vec<LessonPlanData>,
}

#[derive(Deserialize, Debug)]
struct LessonPlanData {
    #[serde(rename(deserialize = "Zawartosc"))]
    content: Vec<LessonContent>,
}

#[derive(Deserialize, Debug)]
struct LessonContent {
    #[serde(rename(deserialize = "Nazwa"))]
    element: String,
}

#[derive(Serialize)]
struct PlanResponse {
    header: Option<String>,
    lessons: Vec<Lesson>,
}

#[derive(Serialize)]
struct Lesson {
    name: String,
    room: Option<String>,
    index: usize,
    cancelled: bool,
    replacement: Option<String>,
}

#[derive(Deserialize, Debug)]
struct LastTestsResponse {
    data: Vec<LastTestsData>,
}

#[derive(Deserialize, Debug)]
struct LastTestsData {
    #[serde(rename(deserialize = "Zawartosc"))]
    content: Vec<LastTestsContent>,
}

#[derive(Deserialize, Debug)]
struct LastTestsContent {
    #[serde(rename(deserialize = "Nazwa"))]
    name: String,
    #[serde(rename(deserialize = "Url"))]
    url: String,
}

#[derive(Serialize)]
#[serde(untagged)]
enum TestsResponse {
    Success(TestsResponseSuccess),
    Failure(TestsResponseFailure),
}

#[derive(Serialize)]
struct TestsResponseSuccess {
    days: Vec<TestsDay>,
}

#[derive(Serialize)]
struct TestsResponseFailure {
    message: String,
}

#[derive(Serialize)]
struct TestsDay {
    date: String,
    tests: Vec<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum SomeResponse {
    Tests(LastTestsResponse),
    Plan(LessonPlanResponse),
}

async fn request_with_bypass(url: &str, auth_info: &AuthInfo) -> Result<SomeResponse> {
    let resp = requests::post(
        url,
        auth_info,
        requests::Host::UonetPlus,
        Option::<Body>::None,
        None,
    )
    .await?;

    let body = body_text(resp.into_body()).await?;

    serde_json::from_str(&body).context("Failed to run request")
}

async fn get_tests(group: Group) -> Result<TestsResponse> {
    let auth_info = match group {
        Group::One => GROUP_ONE_AUTH.lock(),
        Group::Two => GROUP_TWO_AUTH.lock(),
    }
    .await;

    let Ok(SomeResponse::Tests(data)) =
        request_with_bypass(format!("/{}/Start.mvc/GetLastTests", std::env::var("SYMBOL").unwrap()), &auth_info).await else {
            bail!("Invalid response");
        };

    drop(auth_info);

    let Some(first_data) = data.data.get(0) else {
        return Ok(
            TestsResponse::Failure(TestsResponseFailure { message: "You don't have any tests.".to_string() })
            );
    };

    let mut resp = TestsResponseSuccess { days: Vec::new() };

    let mut days_hash: HashSet<&String> = HashSet::new();

    let mut curr_day_index = -1i32;

    for test in &first_data.content {
        let new_day = days_hash.insert(&test.url);

        if days_hash.len() > 2 {
            break;
        }

        if new_day {
            curr_day_index += 1;
        }

        let day = match resp.days.get_mut(curr_day_index as usize) {
            Some(v) => v,
            None => {
                resp.days.push(TestsDay {
                    date: test.url.clone(),
                    tests: Vec::new(),
                });

                &mut resp.days[curr_day_index as usize]
            }
        };

        let split_by = format!(" {} ", test.url);

        let mut split = test.name.split(&split_by);

        let class_name = split.next().unwrap();

        let rest = split.next().unwrap();

        let mut type_split = rest.split(':');

        let test_type = type_split.next().unwrap();

        let final_string = format!("{class_name} - {test_type}");

        day.tests.push(final_string);
    }

    Ok(TestsResponse::Success(resp))
}

async fn get_plan(group: Group) -> Result<PlanResponse> {
    let auth_info = match group {
        Group::One => GROUP_ONE_AUTH.lock(),
        Group::Two => GROUP_TWO_AUTH.lock(),
    }
    .await;

    let Ok(SomeResponse::Plan(data)) =
        request_with_bypass(format!("/{}/Start.mvc/GetKidsLessonPlan", std::env::var("SYMBOL").unwrap()), &auth_info).await else {
            bail!("Invalid response");
        };

    drop(auth_info);

    let mut resp = PlanResponse {
        header: None,
        lessons: Vec::new(),
    };

    let Some(first_data) = data.data.get(0) else {
        resp.header = Some("Brak lekcji.".to_owned());

        return Ok(resp);
    };

    let mut iter = first_data.content.iter();

    while let Some(class) = iter.next() {
        let html = Dom::parse(&class.element)?;

        let element = &html
            .children
            .iter()
            .find(|node| {
                if let Some(el) = node.element() {
                    el.name != "br"
                } else {
                    false
                }
            })
            .context("Missing children.")?
            .element()
            .unwrap();

        let class_list = &element.classes;

        if class_list.contains(&"dayHeader".to_string()) {
            // It's a day header.

            if resp.header.is_some() {
                break;
            }

            let now = chrono::Local::now();

            if now.hour() >= 15 && first_data.content.len() > 11 {
                // Show next day.

                while let Some(class) = iter.next() {
                    println!("Checking element {}", class.element);
                    let html = Dom::parse(&class.element)?;

                    let element = &html
                        .children
                        .iter()
                        .find(|node| {
                            if let Some(el) = node.element() {
                                el.name != "br"
                            } else {
                                false
                            }
                        })
                        .context("Missing children.")?
                        .element()
                        .unwrap();

                    let class_list = &element.classes;

                    if class_list.contains(&"dayHeader".to_string()) {
                        println!("It's a day header");
                        let text = element.children[0]
                            .text()
                            .context("Header didn't have a first text child.")?;

                        resp.header = Some(text.to_owned());
                        break;
                    }
                }
            } else {
                let text = element.children[0]
                    .text()
                    .context("Header didn't have a first text child.")?;

                resp.header = Some(text.to_string());
            }
        } else {
            // Must be a class!

            let index = &element.children[0]
                .text()
                .context("Class element didn't have the index as first value.")?;

            let mut cancelled = false;
            let mut replacement = None;

            println!("{:#?}", html.children);

            let name_and_room = match &html.children[1] {
                Node::Element(el) => {
                    if el.classes.contains(&"striked".to_owned()) {
                        cancelled = true;
                    }
                    el.children[0]
                        .text()
                        .context("Failed to get name and room text")?
                }
                Node::Text(text) => &text[8..],
                _ => unreachable!(),
            };

            let mut name_and_room_text = name_and_room.split(", sala ");

            let annotation_el = html
                .children
                .iter()
                .find(|el| {
                    if let Some(el) = el.element() {
                        el.name == "div" && el.classes.contains(&"annotation".to_owned())
                    } else {
                        false
                    }
                })
                .map(|el| el.element().unwrap());

            if let Some(annotation_el) = annotation_el {
                if !cancelled {
                    for child in &annotation_el.children {
                        if let Some(text) = child.text() {
                            replacement = Some(text[14..(text.len() - 1)].to_owned());
                            break;
                        }
                    }
                }
            }

            resp.lessons.push(Lesson {
                index: index[..(index.len() - 1)].to_owned().parse()?,
                name: name_and_room_text
                    .next()
                    .context("Name missing")?
                    .to_owned(),
                room: name_and_room_text.next().map(|v| v.to_owned()),
                cancelled,
                replacement,
            })
        }
    }

    Ok(resp)
}

#[get("/g1/plan")]
async fn plan_1() -> impl Responder {
    let data = get_plan(Group::One).await;

    match data {
        Err(err) => {
            eprintln!("{:#?}", err);
            "Failed to get plan".to_owned()
        }
        Ok(data) => serde_json::to_string(&data).unwrap_or("Failed to get plan".to_owned()),
    }
}

#[get("/g2/plan")]
async fn plan_2() -> impl Responder {
    let data = get_plan(Group::Two).await;

    match data {
        Err(err) => {
            eprintln!("{:#?}", err);
            "Failed to get plan".to_owned()
        }
        Ok(data) => serde_json::to_string(&data).unwrap_or("Failed to get plan".to_owned()),
    }
}

#[get("/g1/tests")]
async fn tests_1() -> impl Responder {
    let data = get_tests(Group::One).await;

    match data {
        Err(err) => {
            eprintln!("{:#?}", err);
            "Failed to get plan".to_owned()
        }
        Ok(data) => serde_json::to_string(&data).unwrap_or("Failed to get tests".to_owned()),
    }
}

#[get("/g2/tests")]
async fn tests_2() -> impl Responder {
    let data = get_tests(Group::Two).await;

    match data {
        Err(err) => {
            eprintln!("{:#?}", err);
            "Failed to get plan".to_owned()
        }
        Ok(data) => serde_json::to_string(&data).unwrap_or("Failed to get tests".to_owned()),
    }
}

async fn get_calendar(group: Group, replacements: bool) -> Result<String> {
    let mut cache = match group {
        Group::One => GROUP_ONE_CACHE.lock(),
        Group::Two => GROUP_TWO_CACHE.lock(),
    }
    .await;

    if !cache.is_valid() {
        let mut regular_calendar = ICalendar::new("2.0", "ics-rs");
        let mut replacements_calendar = ICalendar::new("2.0", "ics-rs");

        async fn parse_week<'a>(
            weeks_skipped: u32,
            group: &Group,
        ) -> Result<(Vec<Event<'a>>, Vec<Event<'a>>)> {
            let mut regular_events: Vec<Event<'a>> = Vec::new();
            let mut replacement_events: Vec<Event<'a>> = Vec::new();

            let now = Local::now();

            let auth_info = match group {
                Group::One => GROUP_ONE_AUTH.lock(),
                Group::Two => GROUP_TWO_AUTH.lock(),
            }
            .await;

            let data = endpoints::get_week_plan(
                NaiveDate::from_isoywd_opt(
                    now.iso_week().year(),
                    now.iso_week().week() + weeks_skipped,
                    Weekday::Mon,
                )
                .context("Failed to create date for monday")?,
                &auth_info,
            )
            .await?;

            drop(auth_info);

            for (_, row) in data.data.rows.iter().enumerate() {
                for (col_index, col) in row.iter().enumerate() {
                    if col_index == 0 {
                        continue; // Index 0 is always the lesson hour.
                    }

                    if col == "" {
                        continue; // Empty means no lesson.
                    }

                    let hour = row.get(0).unwrap();

                    let date_chars = data
                        .data
                        .headers
                        .get(col_index)
                        .context("No header for current class")?
                        .text
                        .chars();

                    let date = date_chars
                        .skip_while(|char| char != &'>')
                        .skip(1)
                        .collect::<String>();

                    let mut date_elements = date.split(".");

                    let day = date_elements.next().unwrap();
                    let month = date_elements.next().unwrap();
                    let year = date_elements.next().unwrap();

                    let start_hour = format!("{}{}00", &hour[7..9], &hour[10..12]);
                    let end_hour = format!("{}{}00", &hour[18..20], &hour[21..23]);

                    let cancelled = col.contains("x-treelabel-inv");
                    let replacement = col.contains("x-treelabel-zas");

                    let name = col
                        .chars()
                        .skip_while(|char| char != &'>')
                        .skip(1)
                        .skip_while(|char| char != &'>')
                        .skip(1)
                        .take_while(|char| char != &'<')
                        .collect::<String>();

                    if name == "Praktyka zawodowa" {
                        continue;
                    }

                    let html = Dom::parse(&col)?;

                    let content = html.children[0].element().unwrap();

                    let mut has_empty_el = 0;
                    let mut room = content.children[1].element().unwrap();

                    if room.children.is_empty() {
                        has_empty_el = 1;
                        room = content.children[2].element().unwrap();
                    }

                    let room = room
                        .children
                        .get(0)
                        .map(|room| room.text().map(|text| text.to_string()))
                        .flatten();

                    let teacher_og = content
                        .children
                        .get(2 + has_empty_el)
                        .map(|teacher| {
                            teacher.element().map(|el| {
                                el.children
                                    .get(0)
                                    .map(|text| text.text().unwrap().to_string())
                            })
                        })
                        .flatten()
                        .flatten();

                    let start = format!("{}{}{}T{}", year, month, day, start_hour);

                    let mut event = Event::new(
                        start.clone(),
                        Utc::now().format("%Y%m%dT%H%M%S").to_string(),
                    );

                    if let Some(teacher_og) = teacher_og {
                        let mut teacher_words = teacher_og.split(" ").collect::<Vec<_>>();
                        teacher_words.reverse();

                        let teacher = teacher_words.join(" ");

                        event.push(Property::new(
                            format!("ORGANIZER;CN=\"{}\"", teacher),
                            format!(
                                "MAILTO:{}@{}",
                                unidecode::unidecode(
                                    teacher.to_lowercase().replace(" ", ".").as_str()
                                ),
                                std::env::var("SCHOOL_MAIL").unwrap()
                            ),
                        ));
                    }

                    let notes = content.children.last().map(|child| child.text()).flatten();

                    event.push(Property::new("SUMMARY", name));
                    event.push(Property::new("DTSTART", start));
                    event.push(Property::new(
                        "DTEND",
                        format!("{}{}{}T{}", year, month, day, end_hour),
                    ));

                    if cancelled {
                        event.push(Property::new("STATUS", "CANCELLED"));
                    }

                    if let Some(room) = room {
                        event.push(Property::new("LOCATION", room));
                    }

                    if let Some(description) = notes {
                        event.push(Property::new(
                            "DESCRIPTION",
                            description[1..description.len() - 1].to_string(),
                        ));
                    }

                    if replacement {
                        replacement_events.push(event)
                    } else {
                        regular_events.push(event)
                    }
                }
            }

            Ok((regular_events, replacement_events))
        }

        let weeks = vec![
            parse_week(0, &group).await.unwrap(),
            parse_week(1, &group).await.unwrap(),
            parse_week(2, &group).await.unwrap(),
        ];

        for week in weeks {
            for event in week.0 {
                regular_calendar.add_event(event);
            }
            for event in week.1 {
                replacements_calendar.add_event(event);
            }
        }

        {
            let mut buffer = Vec::new();
            regular_calendar.write(&mut buffer)?;
            let string = String::from_utf8(buffer)?;

            cache.regular_calendar = Some(string);
        }

        {
            let mut buffer = Vec::new();
            replacements_calendar.write(&mut buffer)?;
            let string = String::from_utf8(buffer)?;

            cache.replacements_calendar = Some(string);
        }

        cache.last_updated = Some(Local::now());
    }

    if replacements {
        cache
            .replacements_calendar
            .clone()
            .context("Calendar missing")
    } else {
        cache.regular_calendar.clone().context("Calendar missing")
    }
}

#[get("/g1/plan_zastepstwa.ics")]
async fn calendar_replacements_1() -> impl Responder {
    match get_calendar(Group::One, true).await {
        Err(_) => "An unknown error occurred.".to_owned(),
        Ok(value) => value,
    }
}

#[get("/g1/plan.ics")]
async fn calendar_1() -> impl Responder {
    match get_calendar(Group::One, false).await {
        Err(_) => "An unknown error occurred.".to_owned(),
        Ok(value) => value,
    }
}

#[get("/g2/plan_zastepstwa.ics")]
async fn calendar_replacements_2() -> impl Responder {
    match get_calendar(Group::Two, true).await {
        Err(_) => "An unknown error occurred.".to_owned(),
        Ok(value) => value,
    }
}

#[get("/g2/plan.ics")]
async fn calendar_2() -> impl Responder {
    match get_calendar(Group::Two, false).await {
        Err(_) => "An unknown error occurred.".to_owned(),
        Ok(value) => value,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    {
        let mut auth = GROUP_ONE_AUTH.lock().await;
        auth.cookie = fs::read_to_string("/etc/uonetplan/cookie_1")?
            .lines()
            .next()
            .context("cookie file is empty")?
            .to_owned();
    }

    {
        let mut auth = GROUP_TWO_AUTH.lock().await;
        auth.cookie = fs::read_to_string("/etc/uonetplan/cookie_2")?
            .lines()
            .next()
            .context("cookie file is empty")?
            .to_owned();
    }

    let server_task = HttpServer::new(|| {
        App::new()
            .service(plan_1)
            .service(plan_2)
            .service(tests_1)
            .service(tests_2)
            .service(calendar_1)
            .service(calendar_replacements_1)
            .service(calendar_2)
            .service(calendar_replacements_2)
    })
    .disable_signals()
    .bind(("127.0.0.1", 8080))?
    .run();

    let (_, _) = tokio::join!(cookie_refresher::spawn_refresher(), server_task);

    Ok(())
}
