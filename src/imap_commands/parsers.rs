use nom::{
    branch::alt,
    bytes::complete::{tag_no_case, take_while1},
    character::complete::{char, space1},
    combinator::{map, opt},
    error::{context, VerboseError},
    multi::separated_list0,
    sequence::{delimited, separated_pair, tuple},
    IResult,
};

type Res<'a, U> = IResult<&'a str, U, VerboseError<&'a str>>;

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

pub fn fetch_arguments(input: &str) -> Res<FetchArguments> {
    context("fetch_arguments", inner_fetch_arguments)(input)
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
