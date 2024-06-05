use std::error::Error;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use serenity::all::{
    ChannelId, Context, CreateEmbed, CreateMessage, EmbedMessageBuilding, Message, MessageBuilder,
};

#[derive(Serialize)]
struct GraphQLQuery {
    query: String,
    variables: serde_json::Value,
}

#[derive(Deserialize, Debug)]
#[allow(unused)]
struct TopicTag {
    name: String,
    id: String,
    slug: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Question {
    ac_rate: Option<f64>,
    difficulty: String,
    #[allow(unused)]
    freq_bar: Option<f64>,
    frontend_question_id: String,
    #[allow(unused)]
    is_favor: bool,
    #[allow(unused)]
    paid_only: bool,
    #[allow(unused)]
    status: Option<String>,
    title: String,
    #[allow(unused)]
    title_slug: String,
    #[allow(unused)]
    has_video_solution: bool,
    #[allow(unused)]
    has_solution: bool,
    #[allow(unused)]
    topic_tags: Vec<TopicTag>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActiveDailyCodingChallengeQuestion {
    #[allow(unused)]
    date: String,
    #[allow(unused)]
    user_status: Option<String>,
    link: String,
    question: Question,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Data {
    active_daily_coding_challenge_question: ActiveDailyCodingChallengeQuestion,
}

#[derive(Deserialize)]
struct GraphQLResponse {
    data: Data,
}

const URL: &str = "https://leetcode.com";

async fn fetch_daily_question() -> Result<GraphQLResponse, reqwest::Error> {
    let query = r#"
        query questionOfToday {
            activeDailyCodingChallengeQuestion {
                date
                userStatus
                link
                question {
                    acRate
                    difficulty
                    freqBar
                    frontendQuestionId: questionFrontendId
                    isFavor
                    paidOnly: isPaidOnly
                    status
                    title
                    titleSlug
                    hasVideoSolution
                    hasSolution
                    topicTags {
                        name
                        id
                        slug
                    }
                }
            }
        }
    "#;
    let variables = json!({"categorySlug": "", "skip": 0, "limit": 1, "filters": {}});

    let gql_query = GraphQLQuery {
        query: query.to_string(),
        variables,
    };

    let client = Client::new();
    let response = client
        .post(format!("{URL}/graphql"))
        .json(&gql_query)
        .header("Content-Type", "application/json")
        .send()
        .await?;

    let gql_response = response.json::<GraphQLResponse>().await?;
    Ok(gql_response)
}

pub async fn send_leetcode_daily_question_message(
    ctx: &Context,
    thread_id: ChannelId,
) -> Result<Message, Box<dyn Error>> {
    let res = fetch_daily_question().await?;
    let challenge = res.data.active_daily_coding_challenge_question;
    let title = format!(
        "{}. {}",
        challenge.question.frontend_question_id, challenge.question.title
    );
    let url = format!("{}{}", URL, challenge.link);
    let embed = CreateEmbed::default()
        .title(title)
        .url(url)
        .field("Difficulty", challenge.question.difficulty, true)
        .field(
            "Acceptance Rate",
            format!("{:.2}%", challenge.question.ac_rate.unwrap_or_default()),
            true,
        );
    Ok(thread_id
        .send_message(
            ctx,
            CreateMessage::new()
                .content(
                    MessageBuilder::new()
                        .push("Today's ")
                        .push_named_link("LeetCode", "https://leetcode.com/problemset")
                        .push(" Daily question is out @everyone")
                        .build(),
                )
                .embed(embed),
        )
        .await?)
}
