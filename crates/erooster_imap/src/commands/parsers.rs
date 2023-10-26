use std::ops::Bound;

use erooster_deps::{
    nom::{
        branch::alt,
        bytes::complete::{tag_no_case, take_while1},
        character::complete::{char, digit1, space1},
        combinator::{map, opt, recognize},
        error::{context, VerboseError},
        multi::{many0, separated_list0, separated_list1},
        sequence::{delimited, pair, separated_pair, terminated, tuple},
        IResult,
    },
    tracing::{self, instrument},
};

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
                        y.map(|(a, b)| {
                            (
                                a.parse::<u64>().expect("body peek attribute a is a number"),
                                b.parse::<u64>().expect("body peek attribute b is a number"),
                            )
                        }),
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
                        y.map(|(a, b)| {
                            (
                                a.parse::<u64>().expect("body peek attribute a is a number"),
                                b.parse::<u64>().expect("body peek attribute b is a number"),
                            )
                        }),
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
                        y.map(|(a, b)| {
                            (
                                a.parse::<u64>()
                                    .expect("binary peek attribute a is a number"),
                                b.parse::<u64>()
                                    .expect("binary peek attribute b is a number"),
                            )
                        }),
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
                        y.map(|(a, b)| {
                            (
                                a.parse::<u64>().expect("body attribute a is a number"),
                                b.parse::<u64>().expect("body attribute b is a number"),
                            )
                        }),
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
                        y.map(|(a, b)| {
                            (
                                a.parse::<u64>().expect("binary attribute a is a number"),
                                b.parse::<u64>().expect("binary attribute b is a number"),
                            )
                        }),
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

#[derive(Debug, Clone)]
#[allow(clippy::upper_case_acronyms)]
pub enum SearchReturnOption {
    MIN,
    MAX,
    ALL,
    COUNT,
    SAVE,
    Multiple(Vec<SearchReturnOption>),
}

pub type EmailDate = String;
pub type EmailHeader = String;

#[derive(Debug, Clone)]
#[allow(clippy::upper_case_acronyms)]
pub enum SearchProgram {
    ALL,
    ANSWERED,
    DELETED,
    FLAGGED,
    SEEN,
    UNANSWERED,
    UNDELETED,
    UNFLAGGED,
    UNSEEN,
    DRAFT,
    UNDRAFT,
    BCC(String),
    BEFORE(EmailDate),
    BODY(String),
    CC(String),
    FROM(String),
    KEYWORD(String),
    ON(EmailDate),
    SINCE(EmailDate),
    SUBJECT(String),
    TEXT(String),
    TO(String),
    UNKEYWORD(String),
    HEADER(EmailHeader, String),
    LARGER(u64),
    NOT(Box<SearchProgram>),
    OR(Box<SearchProgram>, Box<SearchProgram>),
    SENTBEFORE(EmailDate),
    SENTON(EmailDate),
    SENTSINCE(EmailDate),
    SMALLER(u64),
    UID(Range),
    // These are actually untagged in the ABNF but since we are an enum we need a tag here
    Range(Range),
    AND(Vec<SearchProgram>),
}

#[derive(Debug, Clone)]
pub struct SearchArguments {
    pub return_opts: SearchReturnOption,
    // TODO: Consider using an enum with supported values plus unknown
    pub charset: Option<String>,
    pub program: SearchProgram,
}

#[instrument(skip(input))]
pub fn search_arguments(input: &str) -> Res<SearchArguments> {
    context("search_arguments", inner_search_arguments)(input)
}

enum ProgramWithOrWithoutCharset {
    With(String, SearchProgram),
    Without(SearchProgram),
}

#[instrument(skip(input))]
fn inner_search_arguments(input: &str) -> Res<SearchArguments> {
    context(
        "inner_search_arguments",
        alt((
            map(
                separated_pair(
                    search_return_opts,
                    space1,
                    alt((
                        map(
                            separated_pair(
                                separated_pair(
                                    tag_no_case("CHARSET"),
                                    space1,
                                    terminated(
                                        take_while1(|x: char| x.is_ascii_alphanumeric()),
                                        space1,
                                    ),
                                ),
                                space1,
                                search_program,
                            ),
                            |((_, x), y)| ProgramWithOrWithoutCharset::With(x.to_string(), y),
                        ),
                        map(search_program, |program| {
                            ProgramWithOrWithoutCharset::Without(program)
                        }),
                    )),
                ),
                |(return_opts, program)| match program {
                    ProgramWithOrWithoutCharset::With(charset, program) => SearchArguments {
                        return_opts,
                        program,
                        charset: Some(charset),
                    },
                    ProgramWithOrWithoutCharset::Without(program) => SearchArguments {
                        return_opts,
                        program,
                        charset: None,
                    },
                },
            ),
            map(
                alt((
                    map(
                        separated_pair(
                            separated_pair(
                                tag_no_case("CHARSET"),
                                space1,
                                terminated(
                                    take_while1(|x: char| x.is_ascii_alphanumeric()),
                                    space1,
                                ),
                            ),
                            space1,
                            search_program,
                        ),
                        |((_, x), y)| ProgramWithOrWithoutCharset::With(x.to_string(), y),
                    ),
                    map(search_program, |program| {
                        ProgramWithOrWithoutCharset::Without(program)
                    }),
                )),
                |program| match program {
                    ProgramWithOrWithoutCharset::With(charset, program) => SearchArguments {
                        return_opts: SearchReturnOption::ALL,
                        program,
                        charset: Some(charset),
                    },
                    ProgramWithOrWithoutCharset::Without(program) => SearchArguments {
                        return_opts: SearchReturnOption::ALL,
                        program,
                        charset: None,
                    },
                },
            ),
        )),
    )(input)
}

#[instrument(skip(input))]
fn search_program(input: &str) -> Res<SearchProgram> {
    context(
        "search_program",
        map(separated_list1(space1, search_key), |a| {
            let first_element = a.first().expect("Invalid search program");
            if a.len() > 1 {
                SearchProgram::AND(a)
            } else {
                first_element.clone()
            }
        }),
    )(input)
}

#[instrument(skip(input))]
fn search_key(input: &str) -> Res<SearchProgram> {
    context(
        "search_key",
        alt((
            alt((
                // 1
                map(tag_no_case("ALL"), |_| SearchProgram::ALL),
                // 2
                map(tag_no_case("ANSWERED"), |_| SearchProgram::ANSWERED),
                // 3
                map(tag_no_case("DELETED"), |_| SearchProgram::DELETED),
                // 4
                map(tag_no_case("FLAGGED"), |_| SearchProgram::FLAGGED),
                // 5
                map(tag_no_case("SEEN"), |_| SearchProgram::SEEN),
                // 6
                map(tag_no_case("UNANSWERED"), |_| SearchProgram::UNANSWERED),
                // 7
                map(tag_no_case("UNDELETED"), |_| SearchProgram::UNDELETED),
                // 8
                map(tag_no_case("UNFLAGGED"), |_| SearchProgram::UNFLAGGED),
                // 9
                map(tag_no_case("UNSEEN"), |_| SearchProgram::UNSEEN),
                // 10
                map(tag_no_case("DRAFT"), |_| SearchProgram::DRAFT),
                // 11
                map(tag_no_case("UNDRAFT"), |_| SearchProgram::UNDRAFT),
                // 12
                map(parse_selected_range_inner, |range| {
                    SearchProgram::Range(range)
                }),
                // 13
                map(
                    separated_pair(
                        tag_no_case("BCC"),
                        space1,
                        take_while1(|x: char| x.is_ascii_alphanumeric()),
                    ),
                    |(_, query): (&str, &str)| SearchProgram::BCC(query.to_string()),
                ),
                // 14
                map(
                    separated_pair(
                        tag_no_case("BEFORE"),
                        space1,
                        take_while1(|x: char| x.is_ascii_alphanumeric() || x == '-'),
                    ),
                    |(_, query): (&str, &str)| SearchProgram::BEFORE(query.to_string()),
                ),
                // 15
                map(
                    separated_pair(
                        tag_no_case("BODY"),
                        space1,
                        take_while1(|x: char| x.is_ascii_alphanumeric()),
                    ),
                    |(_, query): (&str, &str)| SearchProgram::BODY(query.to_string()),
                ),
                // 16
                map(
                    separated_pair(
                        tag_no_case("CC"),
                        space1,
                        take_while1(|x: char| x.is_ascii_alphanumeric()),
                    ),
                    |(_, query): (&str, &str)| SearchProgram::CC(query.to_string()),
                ),
                // 17
                map(
                    separated_pair(
                        tag_no_case("FROM"),
                        space1,
                        take_while1(|x: char| x.is_ascii_alphanumeric()),
                    ),
                    |(_, query): (&str, &str)| SearchProgram::FROM(query.to_string()),
                ),
                // 18
                map(
                    separated_pair(
                        tag_no_case("KEYWORD"),
                        space1,
                        take_while1(|x: char| x.is_ascii_alphanumeric()),
                    ),
                    |(_, query): (&str, &str)| SearchProgram::KEYWORD(query.to_string()),
                ),
                // 19
                map(
                    separated_pair(
                        tag_no_case("ON"),
                        space1,
                        take_while1(|x: char| x.is_ascii_alphanumeric() || x == '-'),
                    ),
                    |(_, query): (&str, &str)| SearchProgram::ON(query.to_string()),
                ),
                // 20
                map(
                    separated_pair(
                        tag_no_case("SINCE"),
                        space1,
                        take_while1(|x: char| x.is_ascii_alphanumeric() || x == '-'),
                    ),
                    |(_, query): (&str, &str)| SearchProgram::SINCE(query.to_string()),
                ),
            )),
            alt((
                // 21
                map(
                    separated_pair(
                        tag_no_case("SUBJECT"),
                        space1,
                        take_while1(|x: char| x.is_ascii_alphanumeric()),
                    ),
                    |(_, query): (&str, &str)| SearchProgram::SUBJECT(query.to_string()),
                ),
                // 22
                map(
                    separated_pair(
                        tag_no_case("TEXT"),
                        space1,
                        take_while1(|x: char| x.is_ascii_alphanumeric()),
                    ),
                    |(_, query): (&str, &str)| SearchProgram::TEXT(query.to_string()),
                ),
                // 23
                map(
                    separated_pair(
                        tag_no_case("TO"),
                        space1,
                        take_while1(|x: char| x.is_ascii_alphanumeric()),
                    ),
                    |(_, query): (&str, &str)| SearchProgram::TO(query.to_string()),
                ),
                // 24
                map(
                    separated_pair(
                        tag_no_case("UNKEYWORD"),
                        space1,
                        take_while1(|x: char| x.is_ascii_alphanumeric()),
                    ),
                    |(_, query): (&str, &str)| SearchProgram::UNKEYWORD(query.to_string()),
                ),
                // 25
                map(
                    separated_pair(
                        tag_no_case("HEADER"),
                        space1,
                        separated_pair(
                            take_while1(|x: char| x.is_ascii_alphanumeric()),
                            space1,
                            take_while1(|x: char| x.is_ascii_alphanumeric()),
                        ),
                    ),
                    |(_, (header_name, header_value)): (&str, (&str, &str))| {
                        SearchProgram::HEADER(header_name.to_string(), header_value.to_string())
                    },
                ),
                // 26
                map(
                    separated_pair(
                        tag_no_case("LARGER"),
                        space1,
                        take_while1(|x: char| x.is_ascii_digit()),
                    ),
                    |(_, query): (&str, &str)| {
                        SearchProgram::LARGER(
                            query.parse::<u64>().expect("LARGER query is not u64"),
                        )
                    },
                ),
                // 27
                map(
                    separated_pair(tag_no_case("NOT"), space1, search_key),
                    |(_, query): (&str, SearchProgram)| SearchProgram::NOT(Box::new(query)),
                ),
                // 28
                map(
                    separated_pair(
                        tag_no_case("OR"),
                        space1,
                        separated_pair(search_key, space1, search_key),
                    ),
                    |(_, (a, b)): (&str, (SearchProgram, SearchProgram))| {
                        SearchProgram::OR(Box::new(a), Box::new(b))
                    },
                ),
                // 29
                map(
                    separated_pair(
                        tag_no_case("SENTBEFORE"),
                        space1,
                        take_while1(|x: char| x.is_ascii_alphanumeric() || x == '-'),
                    ),
                    |(_, query): (&str, &str)| SearchProgram::SENTBEFORE(query.to_string()),
                ),
                // 30
                map(
                    separated_pair(
                        tag_no_case("SENTON"),
                        space1,
                        take_while1(|x: char| x.is_ascii_alphanumeric() || x == '-'),
                    ),
                    |(_, query): (&str, &str)| SearchProgram::SENTON(query.to_string()),
                ),
                // 31
                map(
                    separated_pair(
                        tag_no_case("SENTSINCE"),
                        space1,
                        take_while1(|x: char| x.is_ascii_alphanumeric() || x == '-'),
                    ),
                    |(_, query): (&str, &str)| SearchProgram::SENTSINCE(query.to_string()),
                ),
                // 32
                map(
                    separated_pair(
                        tag_no_case("SMALLER"),
                        space1,
                        take_while1(|x: char| x.is_ascii_alphanumeric()),
                    ),
                    |(_, query): (&str, &str)| {
                        SearchProgram::SMALLER(
                            query.parse::<u64>().expect("LARGER query is not u64"),
                        )
                    },
                ),
                // 33
                map(
                    separated_pair(tag_no_case("UID"), space1, parse_selected_range_inner),
                    |(_, query): (&str, Range)| SearchProgram::UID(query),
                ),
                // 34
                map(parse_selected_range_inner, |query: Range| {
                    SearchProgram::Range(query)
                }),
                // 35
                map(
                    delimited(char('('), separated_list1(space1, search_key), char(')')),
                    SearchProgram::AND,
                ),
            )),
        )),
    )(input)
}

#[instrument(skip(input))]
fn search_return_opts(input: &str) -> Res<SearchReturnOption> {
    context(
        "search_return_opts",
        map(
            tuple((
                space1,
                tag_no_case("RETURN"),
                space1,
                delimited(
                    char('('),
                    many0(alt((
                        map(tag_no_case("MIN"), |_| SearchReturnOption::MIN),
                        map(tag_no_case("MAX"), |_| SearchReturnOption::MAX),
                        map(tag_no_case("ALL"), |_| SearchReturnOption::ALL),
                        map(tag_no_case("COUNT"), |_| SearchReturnOption::COUNT),
                        map(tag_no_case("SAVE"), |_| SearchReturnOption::SAVE),
                    ))),
                    char(')'),
                ),
            )),
            |(_, _, _, list)| {
                if list.is_empty() {
                    SearchReturnOption::ALL
                } else if list.len() == 1 {
                    list[0].clone()
                } else {
                    SearchReturnOption::Multiple(list)
                }
            },
        ),
    )(input)
}

#[derive(Debug, Clone)]
pub enum RangeEnd {
    End(u32),
    All,
}

#[derive(Debug, Clone)]
pub enum Range {
    Single(u32),
    Range(u32, RangeEnd),
}

impl std::ops::RangeBounds<u32> for Range {
    fn start_bound(&self) -> Bound<&u32> {
        match self {
            Range::Range(x, _) | Range::Single(x) => Bound::Included(x),
        }
    }

    fn end_bound(&self) -> Bound<&u32> {
        match self {
            Range::Range(_, RangeEnd::All) => Bound::Unbounded,
            Range::Range(_, RangeEnd::End(x)) | Range::Single(x) => Bound::Included(x),
        }
    }
}

impl Range {
    pub fn contains<U>(&self, item: &U) -> bool
    where
        u32: PartialOrd<U>,
        U: ?Sized + PartialOrd<u32>,
    {
        <Self as std::ops::RangeBounds<u32>>::contains(self, item)
    }
}

#[instrument(skip(input))]
pub fn parse_selected_range_inner(input: &str) -> Res<Range> {
    context(
        "parse_selected_range_inner",
        alt((
            map(
                separated_pair(
                    digit1,
                    char(':'),
                    map(alt((tag_no_case("*"), digit1)), |x: &str| match x {
                        "*" => RangeEnd::All,
                        _ => RangeEnd::End(x.parse::<u32>().expect("range end is a number")),
                    }),
                ),
                |(x, y): (&str, RangeEnd)| {
                    Range::Range(x.parse::<u32>().expect("range start is a number"), y)
                },
            ),
            map(digit1, |x: &str| {
                Range::Single(x.parse::<u32>().expect("single range is a number"))
            }),
        )),
    )(input)
}

#[instrument(skip(input))]
pub fn parse_selected_range(input: &str) -> Res<Vec<Range>> {
    context(
        "parse_selected_range",
        separated_list0(char(','), parse_selected_range_inner),
    )(input)
}

#[instrument(skip(input))]
fn day_of_week(input: &str) -> Res<&str> {
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
fn month(input: &str) -> Res<&str> {
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
                    Time(format!("{hours}:{minutes}:{seconds}"))
                } else {
                    Time(format!("{hours}:{minutes}"))
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

pub type AppendArgs<'a> = (Option<Vec<&'a str>>, Option<DateTime>, LiteralSize);

#[instrument(skip(input))]
pub fn append_arguments(input: &str) -> Res<AppendArgs> {
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
                            s.parse::<usize>().expect("literal size is a number")
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

// Parses this ABNF:
//
// date            = date-text / DQUOTE date-text DQUOTE
//
// date-day        = 1*2DIGIT
//                     ; Day of month
//
// date-day-fixed  = (SP DIGIT) / 2DIGIT
//                     ; Fixed-format version of date-day
//
// date-month      = "Jan" / "Feb" / "Mar" / "Apr" / "May" / "Jun" /
//                   "Jul" / "Aug" / "Sep" / "Oct" / "Nov" / "Dec"
//
// date-text       = date-day "-" date-month "-" date-year
//
// date-year       = 4DIGIT
#[instrument(skip(input))]
pub fn parse_search_date(input: &str) -> Res<time::Date> {
    context(
        "parse_search_date",
        map(
            tuple((
                opt(char('"')),
                digit1,
                char('-'),
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
                char('-'),
                digit1,
                opt(char('"')),
            )),
            |(_, day, _, month, _, year, _)| {
                let date = format!("{}-{}-{}", day, month, year);
                time::Date::parse(
                    &date,
                    time::macros::format_description!(
                        "[day]-[month repr:short case_sensitive:false]-[year]"
                    ),
                )
                .expect("date is in correct format")
            },
        ),
    )(input)
}

// Parses this ABNF:
//
// ("+" / "-") 4DIGIT
//
// Signed four-digit value of hhmm representing hours and minutes east of
// Greenwich (that is, the amount that the given time differs from
// Universal Time).  Subtracting the timezone from the given time will give
// the UT form. The Universal Time zone is "+0000".
#[instrument(skip(input))]
fn zone(input: &str) -> Res<&str> {
    context(
        "zone",
        recognize(tuple((alt((tag_no_case("+"), tag_no_case("-"))), digit1))),
    )(input)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use erooster_deps::tokio;

    use super::*;

    #[tokio::test]
    async fn test_section() {
        let input = "[HEADER.FIELDS (From To Cc Bcc Subject Date Message-ID Priority X-Priority References Newsgroups In-Reply-To Content-Type Reply-To x-spamd-result x-spam-score x-rspamd-score x-spam-status x-mailscanner-spamcheck X-Spam-Flag x-spam-level)]";
        let args = section(input);
        println!("{args:?}");
        assert!(args.is_ok());
        let (unparsed, _) = args.unwrap();
        assert_eq!(unparsed, "");
    }

    #[tokio::test]
    async fn test_section_text() {
        let input = "HEADER.FIELDS (From To Cc Bcc Subject Date Message-ID Priority X-Priority References Newsgroups In-Reply-To Content-Type Reply-To x-spamd-result x-spam-score x-rspamd-score x-spam-status x-mailscanner-spamcheck X-Spam-Flag x-spam-level)";
        let args = section_text(input);
        println!("{args:?}");
        assert!(args.is_ok());
        let (unparsed, _) = args.unwrap();
        assert_eq!(unparsed, "");
    }

    #[tokio::test]
    async fn test_fetch_attributes() {
        let input = "BODY.PEEK[HEADER.FIELDS (From To Cc Bcc Subject Date Message-ID Priority X-Priority References Newsgroups In-Reply-To Content-Type Reply-To x-spamd-result x-spam-score x-rspamd-score x-spam-status x-mailscanner-spamcheck X-Spam-Flag x-spam-level)]";
        let args = fetch_attributes(input);
        println!("{args:?}");
        assert!(args.is_ok());
        let (unparsed, _) = args.unwrap();
        assert_eq!(unparsed, "");
    }

    #[tokio::test]
    async fn test_inner_fetch_arguments() {
        let input = "(UID RFC822.SIZE FLAGS BODY.PEEK[HEADER.FIELDS (From To Cc Bcc Subject Date Message-ID Priority X-Priority References Newsgroups In-Reply-To Content-Type Reply-To x-spamd-result x-spam-score x-rspamd-score x-spam-status x-mailscanner-spamcheck X-Spam-Flag x-spam-level)])";
        let args = inner_fetch_arguments(input);
        println!("{args:?}");
        assert!(args.is_ok());
        let (unparsed, _) = args.unwrap();
        assert_eq!(unparsed, "");
    }

    #[tokio::test]
    async fn test_fetch_arguments() {
        let input = "UID RFC822.SIZE FLAGS BODY.PEEK[HEADER.FIELDS (From To Cc Bcc Subject Date Message-ID Priority X-Priority References Newsgroups In-Reply-To Content-Type Reply-To x-spamd-result x-spam-score x-rspamd-score x-spam-status x-mailscanner-spamcheck X-Spam-Flag x-spam-level)]";
        let args = fetch_arguments(input);
        println!("{args:?}");
        assert!(args.is_ok());
        let (unparsed, _) = args.unwrap();
        assert_eq!(unparsed, "");
    }
}
