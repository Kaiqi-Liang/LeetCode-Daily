use rand::{prelude::SliceRandom, thread_rng};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use serenity::all::{
    ChannelId, Colour, Context, CreateEmbed, CreateMessage, EmbedMessageBuilding, Message,
    MessageBuilder,
};
use std::error::Error;

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
struct ProblemsetQuestionList {
    #[allow(unused)]
    total: u16,
    questions: Vec<Question>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActiveDailyCodingChallengeQuestionData {
    active_daily_coding_challenge_question: ActiveDailyCodingChallengeQuestion,
}

#[derive(Deserialize)]
struct ActiveDailyCodingChallengeQuestionResponse {
    data: ActiveDailyCodingChallengeQuestionData,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProblemsetQuestionListData {
    problemset_question_list: ProblemsetQuestionList,
}

#[derive(Deserialize)]
struct ProblemsetQuestionListResponse {
    data: ProblemsetQuestionListData,
}

const URL: &str = "https://leetcode.com";

async fn fetch_daily_question() -> Result<ActiveDailyCodingChallengeQuestionResponse, reqwest::Error>
{
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
    let gql_query = GraphQLQuery {
        query: query.to_string(),
        variables: serde_json::Value::default(),
    };
    Client::new()
        .post(format!("{URL}/graphql"))
        .json(&gql_query)
        .header("Content-Type", "application/json")
        .send()
        .await?
        .json::<ActiveDailyCodingChallengeQuestionResponse>()
        .await
}

async fn fetch_all_questions() -> Result<ProblemsetQuestionListResponse, reqwest::Error> {
    let query = r#"
        query problemsetQuestionList($categorySlug: String, $limit: Int, $skip: Int, $filters: QuestionListFilterInput) {
            problemsetQuestionList: questionList(
                categorySlug: $categorySlug
                limit: $limit
                skip: $skip
                filters: $filters
            ) {
                total: totalNum
                questions: data {
                    acRate
                    difficulty
                    freqBar
                    frontendQuestionId: questionFrontendId
                    isFavor
                    paidOnly: isPaidOnly
                    status
                    title
                    titleSlug
                    topicTags {
                        name
                        id
                        slug
                    }
                    hasSolution
                    hasVideoSolution
                }
            }
        }
    "#;
    let gql_query = GraphQLQuery {
        query: query.to_string(),
        variables: json!({"categorySlug": "", "skip": 0, "limit": 3000, "filters": {}}),
    };
    Client::new()
        .post(format!("{URL}/graphql"))
        .json(&gql_query)
        .header("Content-Type", "application/json")
        .send()
        .await?
        .json::<ProblemsetQuestionListResponse>()
        .await
}

fn create_embed(question: &Question, link: String) -> CreateEmbed {
    let title = format!("{}. {}", question.frontend_question_id, question.title);
    let url = format!("{}{}", URL, link);
    let colour = match question.difficulty.as_str() {
        "Easy" => Colour::DARK_GREEN,
        "Medium" => Colour::ORANGE,
        "Hard" => Colour::DARK_RED,
        _ => Colour::default(),
    };
    CreateEmbed::default()
        .title(title)
        .url(url)
        .colour(colour)
        .field("Difficulty", question.difficulty.clone(), true)
        .field(
            "Acceptance Rate",
            format!("{:.2}%", question.ac_rate.unwrap_or_default()),
            true,
        )
}

pub async fn send_leetcode_daily_question_message(
    ctx: &Context,
    thread_id: ChannelId,
) -> Result<Message, Box<dyn Error>> {
    let challenge = fetch_daily_question()
        .await?
        .data
        .active_daily_coding_challenge_question;
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
                .embed(create_embed(&challenge.question, challenge.link)),
        )
        .await?)
}

pub async fn send_random_leetcode_question_message(
    ctx: &Context,
    thread_id: ChannelId,
) -> Result<Message, Box<dyn Error>> {
    let questions = fetch_all_questions()
        .await?
        .data
        .problemset_question_list
        .questions;
    let question = questions
        .choose(&mut thread_rng())
        .expect("questions.len() > 0");
    Ok(thread_id
        .send_message(
            ctx,
            CreateMessage::new()
                .content(
                    MessageBuilder::new()
                        .push("Here's a random question from ")
                        .push_named_link("LeetCode", "https://leetcode.com/problemset")
                        .build(),
                )
                .embed(create_embed(
                    question,
                    format!("/problems/{}", question.title.replace(' ', "-")),
                )),
        )
        .await?)
}
