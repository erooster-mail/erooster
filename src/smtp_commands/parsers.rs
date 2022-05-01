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
    println!("localpart_arguments Input: {}", input);
    context(
        "localpart_arguments",
        delimited(tag("<"), take_while1(|c: char| c != ','), tag(">")),
    )(input)
    .map(|(x, y)| (x, y))
}
