// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

use erooster_deps::{
    nom::{
        bytes::complete::take_while1,
        character::complete::char,
        error::{context, VerboseError},
        multi::many0,
        sequence::delimited,
        IResult,
    },
    tracing::{self, instrument},
};

type Res<'a, U> = IResult<&'a str, U, VerboseError<&'a str>>;

// TODO parse relay vs no relay
#[instrument(skip(input))]
fn localpart(input: &str) -> Res<Vec<&str>> {
    context(
        "localpart",
        many0(take_while1(|c: char| c != ',' && c != '>')),
    )(input)
    .map(|(x, y)| (x, y))
}

#[instrument(skip(input))]
pub fn localpart_arguments(input: &str) -> Res<Vec<&str>> {
    context(
        "localpart_arguments",
        delimited(char('<'), localpart, char('>')),
    )(input)
    .map(|(x, y)| (x, y))
}
