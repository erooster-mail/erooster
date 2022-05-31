use nom::{
    branch::alt,
    bytes::complete::{tag_no_case, take_while1},
    character::complete::{char, space1},
    combinator::{map, opt},
    error::{context, VerboseError},
    multi::{many0, separated_list0},
    sequence::{delimited, separated_pair, tuple},
    IResult,
};

type Res<'a, U> = IResult<&'a str, U, VerboseError<&'a str>>;
fn header_list(input: &str) -> Res<Vec<&str>> {
    context(
        "header_list",
        delimited(
            char('('),
            many0(take_while1(|c: char| !c.is_whitespace() && c != ')')),
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

fn section_text(input: &str) -> Res<SectionText> {
    context(
        "section_text",
        alt((
            map(tag_no_case("Header"), |_| SectionText::Header),
            map(tag_no_case("Text"), |_| SectionText::Text),
            map(
                separated_pair(tag_no_case("Header.Fields"), space1, header_list),
                |(_, x)| SectionText::HeaderFields(x.iter().map(ToString::to_string).collect()),
            ),
            map(
                separated_pair(tag_no_case("Header.Fields.Not"), space1, header_list),
                |(_, x)| SectionText::HeaderFieldsNot(x.iter().map(ToString::to_string).collect()),
            ),
        )),
    )(input)
}

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
                    space1,
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
                |(_, _, x, _, y)| {
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
                    space1,
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
                |(_, _, x, _, y)| {
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
                    space1,
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
                |(_, _, x, _, y)| {
                    FetchAttributes::BinaryPeek(
                        x,
                        y.map(|(a, b)| (a.parse::<u64>().unwrap(), b.parse::<u64>().unwrap())),
                    )
                },
            ),
            map(
                tuple((tag_no_case("BINARY.SIZE"), space1, section)),
                |(_, _, x)| FetchAttributes::BinarySize(x),
            ),
            map(
                tuple((
                    tag_no_case("BODY"),
                    section,
                    space1,
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
                |(_, _, x, _, y)| {
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
                    space1,
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
                |(_, _, x, _, y)| {
                    FetchAttributes::Binary(
                        x,
                        y.map(|(a, b)| (a.parse::<u64>().unwrap(), b.parse::<u64>().unwrap())),
                    )
                },
            ),
            map(
                separated_pair(tag_no_case("BODY"), space1, tag_no_case("[STRUCTURE]")),
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

fn inner_fetch_arguments(input: &str) -> Res<FetchArguments> {
    context(
        "inner_fetch_arguments",
        alt((
            map(tag_no_case("all"), |_| FetchArguments::All),
            map(tag_no_case("fast"), |_| FetchArguments::Fast),
            map(tag_no_case("full"), |_| FetchArguments::Full),
            map(
                separated_list0(space1, fetch_attributes),
                FetchArguments::List,
            ),
            map(fetch_attributes, FetchArguments::Single),
            map(
                delimited(
                    char('('),
                    separated_list0(space1, fetch_attributes),
                    char(')'),
                ),
                FetchArguments::List,
            ),
        )),
    )(input)
}

// This has a weird type signature due to lifetime conflicts on the caller
#[allow(clippy::needless_pass_by_value)]
pub fn fetch_arguments(
    input: String,
) -> color_eyre::eyre::Result<(String, FetchArguments), String> {
    context(
        "fetch_arguments",
        delimited(char('('), inner_fetch_arguments, char(')')),
    )(&input)
    .map(|(x, y)| (x.to_string(), y))
    .map_err(|e| e.to_string())
}
