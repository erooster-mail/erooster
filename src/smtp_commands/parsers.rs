use nom::{
    bytes::complete::{tag, take_while1},
    error::{context, VerboseError},
    multi::many0,
    sequence::delimited,
    IResult,
};

type Res<'a, U> = IResult<&'a str, U, VerboseError<&'a str>>;

// TODO parse relay vs no relay
pub fn localpart_arguments(input: &str) -> Res<Vec<&str>> {
    context(
        "localpart_arguments",
        delimited(tag("<"), many0(take_while1(|c: char| c != ',')), tag(">")),
    )(input)
    .map(|(x, y)| (x, y))
}
