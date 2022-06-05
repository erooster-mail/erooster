use nom::{
    branch::alt,
    bytes::complete::{tag_no_case, take_while1},
    character::complete::{char, digit1, space1},
    combinator::{map, opt},
    error::{context, VerboseError},
    multi::separated_list0,
    sequence::{delimited, pair, separated_pair, tuple},
    IResult,
};
use tracing::instrument;

type Res<'a, U> = IResult<&'a str, U, VerboseError<&'a str>>;

#[instrument(skip(input))]
fn header_list(input: &str) -> Res<Vec<&str>> {
    context(
        "header_list",
        delimited(
            char('('),
            separated_list0(
                space1,
                take_while1(|c: char| c != ')' && !c.is_whitespace()),
            ),
            char(')'),
        ),
    )(input)
}

#[derive(Debug, Clone)]
pub enum SectionText {
    Header,
    Text,
    HeaderFields(Vec<String>),
    HeaderFieldsNot(Vec<String>),
}

#[instrument(skip(input))]
fn section_text(input: &str) -> Res<SectionText> {
    context(
        "section_text",
        alt((
            map(
                separated_pair(tag_no_case("Header.Fields"), space1, header_list),
                |(_, x)| SectionText::HeaderFields(x.iter().map(ToString::to_string).collect()),
            ),
            map(
                separated_pair(tag_no_case("Header.Fields.Not"), space1, header_list),
                |(_, x)| SectionText::HeaderFieldsNot(x.iter().map(ToString::to_string).collect()),
            ),
            map(tag_no_case("Header"), |_| SectionText::Header),
            map(tag_no_case("Text"), |_| SectionText::Text),
        )),
    )(input)
}

#[instrument(skip(input))]
fn section(input: &str) -> Res<Option<SectionText>> {
    context(
        "section",
        delimited(char('['), opt(section_text), char(']')),
    )(input)
}

#[derive(Debug, Clone)]
pub enum FetchAttributes {
    Envelope,
    Flags,
    InternalDate,
    RFC822Size,
    // TODO remove this with imap4rev2 adaption in clients or feature flag
    RFC822Header,
    Uid,
    BodyStructure,
    BodySection(Option<SectionText>, Option<(u64, u64)>),
    BodyPeek(Option<SectionText>, Option<(u64, u64)>),
    Binary(Option<SectionText>, Option<(u64, u64)>),
    BinaryPeek(Option<SectionText>, Option<(u64, u64)>),
    BinarySize(Option<SectionText>),
}

#[allow(clippy::too_many_lines)]
#[instrument(skip(input))]
fn fetch_attributes(input: &str) -> Res<FetchAttributes> {
    context(
        "fetch_attributes",
        alt((
            map(tag_no_case("ENVELOPE"), |_| FetchAttributes::Envelope),
            map(tag_no_case("FLAGS"), |_| FetchAttributes::Flags),
            map(tag_no_case("INTERNALDATE"), |_| {
                FetchAttributes::InternalDate
            }),
            map(tag_no_case("RFC822.SIZE"), |_| FetchAttributes::RFC822Size),
            map(tag_no_case("RFC822.HEADER"), |_| {
                FetchAttributes::RFC822Header
            }),
            map(tag_no_case("UID"), |_| FetchAttributes::Uid),
            map(
                tuple((
                    tag_no_case("BODY.PEEK"),
                    section,
                    opt(space1),
                    opt(delimited(
                        char('<'),
                        separated_pair(
                            take_while1(|x: char| x.is_ascii_digit()),
                            char('.'),
                            take_while1(|x: char| x.is_ascii_digit()),
                        ),
                        char('>'),
                    )),
                )),
                |(_, x, _, y)| {
                    FetchAttributes::BodyPeek(
                        x,
                        y.map(|(a, b)| (a.parse::<u64>().unwrap(), b.parse::<u64>().unwrap())),
                    )
                },
            ),
            map(
                tuple((
                    tag_no_case("RFC822.PEEK"),
                    section,
                    opt(space1),
                    opt(delimited(
                        char('<'),
                        separated_pair(
                            take_while1(|x: char| x.is_ascii_digit()),
                            char('.'),
                            take_while1(|x: char| x.is_ascii_digit()),
                        ),
                        char('>'),
                    )),
                )),
                |(_, x, _, y)| {
                    FetchAttributes::BodyPeek(
                        x,
                        y.map(|(a, b)| (a.parse::<u64>().unwrap(), b.parse::<u64>().unwrap())),
                    )
                },
            ),
            map(
                tuple((
                    tag_no_case("BINARY.PEEK"),
                    section,
                    opt(space1),
                    opt(delimited(
                        char('<'),
                        separated_pair(
                            take_while1(|x: char| x.is_ascii_digit()),
                            char('.'),
                            take_while1(|x: char| x.is_ascii_digit()),
                        ),
                        char('>'),
                    )),
                )),
                |(_, x, _, y)| {
                    FetchAttributes::BinaryPeek(
                        x,
                        y.map(|(a, b)| (a.parse::<u64>().unwrap(), b.parse::<u64>().unwrap())),
                    )
                },
            ),
            map(tuple((tag_no_case("BINARY.SIZE"), section)), |(_, x)| {
                FetchAttributes::BinarySize(x)
            }),
            map(
                tuple((
                    tag_no_case("BODY"),
                    section,
                    opt(space1),
                    opt(delimited(
                        char('<'),
                        separated_pair(
                            take_while1(|x: char| x.is_ascii_digit()),
                            char('.'),
                            take_while1(|x: char| x.is_ascii_digit()),
                        ),
                        char('>'),
                    )),
                )),
                |(_, x, _, y)| {
                    FetchAttributes::BodySection(
                        x,
                        y.map(|(a, b)| (a.parse::<u64>().unwrap(), b.parse::<u64>().unwrap())),
                    )
                },
            ),
            map(
                tuple((
                    tag_no_case("BINARY"),
                    section,
                    opt(space1),
                    opt(delimited(
                        char('<'),
                        separated_pair(
                            take_while1(|x: char| x.is_ascii_digit()),
                            char('.'),
                            take_while1(|x: char| x.is_ascii_digit()),
                        ),
                        char('>'),
                    )),
                )),
                |(_, x, _, y)| {
                    FetchAttributes::Binary(
                        x,
                        y.map(|(a, b)| (a.parse::<u64>().unwrap(), b.parse::<u64>().unwrap())),
                    )
                },
            ),
            map(
                separated_pair(tag_no_case("BODY"), opt(space1), tag_no_case("[STRUCTURE]")),
                |_| FetchAttributes::BodyStructure,
            ),
            map(tag_no_case("BODY.PEEK"), |_| {
                FetchAttributes::BinaryPeek(None, None)
            }),
            map(tag_no_case("RFC822.PEEK"), |_| {
                FetchAttributes::BinaryPeek(None, None)
            }),
            map(tag_no_case("BODY"), |_| {
                FetchAttributes::BodySection(None, None)
            }),
        )),
    )(input)
}

#[derive(Debug, Clone)]
pub enum FetchArguments {
    All,
    Fast,
    Full,
    Single(FetchAttributes),
    List(Vec<FetchAttributes>),
}

#[instrument(skip(input))]
fn inner_fetch_arguments(input: &str) -> Res<FetchArguments> {
    context(
        "inner_fetch_arguments",
        alt((
            map(tag_no_case("all"), |_| FetchArguments::All),
            map(tag_no_case("fast"), |_| FetchArguments::Fast),
            map(tag_no_case("full"), |_| FetchArguments::Full),
            map(
                delimited(
                    char('('),
                    separated_list0(space1, fetch_attributes),
                    char(')'),
                ),
                FetchArguments::List,
            ),
            map(
                separated_list0(space1, fetch_attributes),
                FetchArguments::List,
            ),
            map(fetch_attributes, FetchArguments::Single),
        )),
    )(input)
}

#[instrument(skip(input))]
pub fn fetch_arguments(input: &str) -> Res<FetchArguments> {
    context("fetch_arguments", inner_fetch_arguments)(input)
}

#[derive(Debug)]
pub enum RangeEnd {
    End(i64),
    All,
}

#[derive(Debug)]
pub enum Range {
    Single(i64),
    Range(i64, RangeEnd),
}

#[instrument(skip(input))]
pub fn parse_selected_range(input: &str) -> Res<Vec<Range>> {
    context(
        "parse_selected_range",
        separated_list0(
            char(','),
            alt((
                map(
                    separated_pair(
                        digit1,
                        char(':'),
                        map(alt((tag_no_case("*"), digit1)), |x: &str| match x {
                            "*" => RangeEnd::All,
                            _ => RangeEnd::End(x.parse::<i64>().unwrap()),
                        }),
                    ),
                    |(x, y): (&str, RangeEnd)| Range::Range(x.parse::<i64>().unwrap(), y),
                ),
                map(digit1, |x: &str| Range::Single(x.parse::<i64>().unwrap())),
            )),
        ),
    )(input)
}

#[instrument(skip(input))]
pub fn day_of_week(input: &str) -> Res<&str> {
    context(
        "day_of_week",
        alt((
            tag_no_case("Mon"),
            tag_no_case("Tue"),
            tag_no_case("Wed"),
            tag_no_case("Thu"),
            tag_no_case("Fri"),
            tag_no_case("Sat"),
            tag_no_case("Sun"),
        )),
    )(input)
}

#[instrument(skip(input))]
pub fn month(input: &str) -> Res<&str> {
    context(
        "month",
        alt((
            tag_no_case("Jan"),
            tag_no_case("Feb"),
            tag_no_case("Mar"),
            tag_no_case("Apr"),
            tag_no_case("May"),
            tag_no_case("Jun"),
            tag_no_case("Jul"),
            tag_no_case("Aug"),
            tag_no_case("Sep"),
            tag_no_case("Oct"),
            tag_no_case("Nov"),
            tag_no_case("Dec"),
        )),
    )(input)
}

struct Time(String);

#[instrument(skip(input))]
fn time(input: &str) -> Res<Time> {
    context(
        "time",
        map(
            tuple((
                digit1,
                tag_no_case(":"),
                digit1,
                opt(pair(tag_no_case(":"), digit1)),
            )),
            |(hours, _, minutes, seconds)| {
                if let Some((_, seconds)) = seconds {
                    Time(format!("{}:{}:{}", hours, minutes, seconds))
                } else {
                    Time(format!("{}:{}", hours, minutes))
                }
            },
        ),
    )(input)
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum DateTime {
    DayName(String),
    DateTime(String),
}

#[instrument(skip(input))]
fn date_time(input: &str) -> Res<DateTime> {
    context(
        "date_time",
        map(
            tuple((
                opt(tuple((day_of_week, tag_no_case(","), space1))),
                digit1,
                space1,
                month,
                space1,
                digit1,
                space1,
                time,
                space1,
                tuple((alt((tag_no_case("+"), tag_no_case("-"))), digit1)),
            )),
            |(day_of_week, day, _, month, _, year, _, time, _, (zone_dir, zone_offset))| {
                if let Some((dayname, _, _)) = day_of_week {
                    DateTime::DayName(format!(
                        "{}, {} {} {} {} {}{}",
                        dayname, day, month, year, time.0, zone_dir, zone_offset
                    ))
                } else {
                    DateTime::DateTime(format!(
                        "{} {} {} {} {}{}",
                        day, month, year, time.0, zone_dir, zone_offset
                    ))
                }
            },
        ),
    )(input)
}

pub struct LiteralSize {
    pub length: usize,
    pub continuation: bool,
}

#[instrument(skip(input))]
pub fn append_arguments(input: &str) -> Res<(Option<Vec<&str>>, Option<DateTime>, LiteralSize)> {
    context(
        "append_arguments",
        map(
            tuple((
                opt(delimited(
                    char('('),
                    separated_list0(
                        space1,
                        take_while1(|c: char| c != ')' && !c.is_whitespace()),
                    ),
                    char(')'),
                )),
                opt(space1),
                opt(date_time),
                opt(space1),
                opt(tag_no_case("UTF8")),
                opt(space1),
                opt(tag_no_case("(")),
                opt(tag_no_case("~")),
                tag_no_case("{"),
                map(
                    pair(
                        map(take_while1(|c: char| c.is_ascii_digit()), |s: &str| {
                            s.parse::<usize>().unwrap()
                        }),
                        map(opt(tag_no_case("+")), |x| x.is_some()),
                    ),
                    |(length, continuation)| LiteralSize {
                        length,
                        continuation,
                    },
                ),
                tag_no_case("}"),
            )),
            |(flags, _, datetime, _, _, _, _, _, _, literal, _)| (flags, datetime, literal),
        ),
    )(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_section() {
        let input = "[HEADER.FIELDS (From To Cc Bcc Subject Date Message-ID Priority X-Priority References Newsgroups In-Reply-To Content-Type Reply-To x-spamd-result x-spam-score x-rspamd-score x-spam-status x-mailscanner-spamcheck X-Spam-Flag x-spam-level)]";
        let args = section(input);
        println!("{:?}", args);
        assert!(args.is_ok());
        let (unparsed, _) = args.unwrap();
        assert_eq!(unparsed, "");
    }

    #[tokio::test]
    async fn test_section_text() {
        let input = "HEADER.FIELDS (From To Cc Bcc Subject Date Message-ID Priority X-Priority References Newsgroups In-Reply-To Content-Type Reply-To x-spamd-result x-spam-score x-rspamd-score x-spam-status x-mailscanner-spamcheck X-Spam-Flag x-spam-level)";
        let args = section_text(input);
        println!("{:?}", args);
        assert!(args.is_ok());
        let (unparsed, _) = args.unwrap();
        assert_eq!(unparsed, "");
    }

    #[tokio::test]
    async fn test_fetch_attributes() {
        let input = "BODY.PEEK[HEADER.FIELDS (From To Cc Bcc Subject Date Message-ID Priority X-Priority References Newsgroups In-Reply-To Content-Type Reply-To x-spamd-result x-spam-score x-rspamd-score x-spam-status x-mailscanner-spamcheck X-Spam-Flag x-spam-level)]";
        let args = fetch_attributes(input);
        println!("{:?}", args);
        assert!(args.is_ok());
        let (unparsed, _) = args.unwrap();
        assert_eq!(unparsed, "");
    }

    #[tokio::test]
    async fn test_inner_fetch_arguments() {
        let input = "(UID RFC822.SIZE FLAGS BODY.PEEK[HEADER.FIELDS (From To Cc Bcc Subject Date Message-ID Priority X-Priority References Newsgroups In-Reply-To Content-Type Reply-To x-spamd-result x-spam-score x-rspamd-score x-spam-status x-mailscanner-spamcheck X-Spam-Flag x-spam-level)])";
        let args = inner_fetch_arguments(input);
        println!("{:?}", args);
        assert!(args.is_ok());
        let (unparsed, _) = args.unwrap();
        assert_eq!(unparsed, "");
    }

    #[tokio::test]
    async fn test_fetch_arguments() {
        let input = "UID RFC822.SIZE FLAGS BODY.PEEK[HEADER.FIELDS (From To Cc Bcc Subject Date Message-ID Priority X-Priority References Newsgroups In-Reply-To Content-Type Reply-To x-spamd-result x-spam-score x-rspamd-score x-spam-status x-mailscanner-spamcheck X-Spam-Flag x-spam-level)]";
        let args = fetch_arguments(input);
        println!("{:?}", args);
        assert!(args.is_ok());
        let (unparsed, _) = args.unwrap();
        assert_eq!(unparsed, "");
    }
}
