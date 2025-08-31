use crate::util::tuple_map::TupleMap;
use chrono::{Duration, NaiveTime};
use nom::{
    Finish, IResult,
    branch::alt,
    bytes::complete::tag,
    character::complete::{self as character, space0 as spc},
    combinator::{eof, map},
    error::{ErrorKind, make_error},
    sequence::{delimited, preceded, terminated, tuple},
};
use nom_regex::str::re_find;
use regex::Regex;
use std::{ops::RangeBounds, str::FromStr, sync::LazyLock};

pub fn dbg_dmp<'a, F, O, E: std::fmt::Debug>(
    mut f: F,
    context: &'static str,
) -> impl FnMut(&'a str) -> IResult<&'a str, O, E>
where
    F: FnMut(&'a str) -> IResult<&'a str, O, E>,
{
    #[cfg(debug_assertions)]
    return move |i: &'a str| match f(i) {
        Err(e) => {
            eprintln!("{}: Error({:?}) at:\n{}", context, e, i);
            Err(e)
        }
        a => a,
    };
    #[cfg(not(debug_assertions))]
    f
}

macro_rules! d {
    ($e:expr) => {
        dbg_dmp($e, concat!("call ", stringify!($e)))
    };
}

fn day_tag(s: &str) -> IResult<&str, &str> {
    alt((tag("day"), tag("dia")))(s)
}

fn at_tag(s: &str) -> IResult<&str, &str> {
    alt((tag("at"), tag("@"), tag("as"), tag("às")))(s)
}

fn spaced<'s, P, O, E>(p: P) -> impl FnMut(&'s str) -> IResult<&'s str, O, E>
where
    P: nom::Parser<&'s str, O, E>,
    E: nom::error::ParseError<&'s str>,
{
    delimited(spc, p, spc)
}

fn parse_number<R: RangeBounds<u16>>(range: R) -> impl FnMut(&str) -> IResult<&str, u16> {
    move |s| match map(character::digit1, |s: &str| s.parse::<u16>())(s) {
        Ok((a, Ok(i))) if range.contains(&i) => Ok((a, i)),
        Ok(_) => Err(nom::Err::Error(make_error(s, ErrorKind::Satisfy))),
        Err(e) => Err(e),
    }
}

fn preceded_number<'s, R: RangeBounds<u16>>(
    tag_str: &'static str,
    range: R,
) -> impl FnMut(&'s str) -> IResult<&'s str, Option<u16>> {
    move |input| match alt((
        preceded(tag(tag_str), character::digit1),
        alt((tag(" "), eof)),
    ))(input)?
    {
        (_, " " | "") => Ok((input, None)),
        (a, digit) => match digit.parse() {
            Ok(i) if range.contains(&i) => Ok((a, Some(i))),
            _ => Err(nom::Err::Error(make_error(input, ErrorKind::Satisfy))),
        },
    }
}

fn date(input: &str) -> IResult<&str, PartialDate> {
    let (input, day) = d!(parse_number(1..=31))(input)?.map_snd(u32::from);
    let (input, month) = d!(preceded_number("/", 1..=12))(input)?.map_snd(|o| o.map(u32::from));
    let (input, year) = d!(preceded_number("/", 0..))(input)?.map_snd(|o| o.map(i32::from));

    Ok((input, PartialDate { day, month, year }))
}

fn time(input: &str) -> IResult<&str, NaiveTime> {
    let (input, hour) = d!(parse_number(0..24))(input)?;
    let (input, min) = d!(preceded_number(":", 0..60))(input)?.map_snd(Option::unwrap_or_default);
    let (input, sec) = d!(preceded_number(":", 0..60))(input)?.map_snd(Option::unwrap_or_default);
    Ok((
        input,
        NaiveTime::from_hms_opt(u32::from(hour), u32::from(min), u32::from(sec)).unwrap(),
    ))
}

fn duration(input: &str) -> IResult<&str, Duration> {
    macro_rules! pat {
        ($($name:ident = $pat:expr;)*) => {$(
            static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pat).unwrap());
        )*};
    }
    pat! {
        SECONDS = "^(s|sec|secs|seconds?|segundos?)(\\s|$)";
        MINUTES = "^(m|min|mins|minutes?|minutos?)(\\s|$)";
        HOURS = "^(h|hours?|horas?)(\\s|$)";
        DAYS = "^(d|days?|dias?)(\\s|$)";
        WEEKS = "^(w|weeks?|semanas?)(\\s|$)";
        MONTHS = "^(months?|mes(es)?)(\\s|$)";
        YEARS = "^(y|years?|anos?)(\\s|$)";
    };
    let re = |r: &LazyLock<Regex>| re_find((*r).clone());

    let (input, amt) = terminated(parse_number(..), spc)(input)?.map_snd(i64::from);
    let (input, dur) = alt((
        map(re(&SECONDS), |_| Duration::seconds(amt)),
        map(re(&MINUTES), |_| Duration::minutes(amt)),
        map(re(&HOURS), |_| Duration::hours(amt)),
        map(re(&DAYS), |_| Duration::days(amt)),
        map(re(&WEEKS), |_| Duration::weeks(amt)),
        map(re(&MONTHS), |_| Duration::days(30 * amt)),
        map(re(&YEARS), |_| Duration::days(365 * amt)),
    ))(input)?;
    Ok((input, dur))
}

fn at_time(input: &str) -> IResult<&str, NaiveTime> {
    preceded(at_tag, spaced(time))(input)
}

fn on_day(input: &str) -> IResult<&str, (PartialDate, NaiveTime)> {
    let (input, (_, date, _, time)) = tuple((
        spaced(d!(day_tag)),
        spaced(d!(date)),
        spaced(d!(at_tag)),
        spaced(d!(time)),
    ))(input)?;

    Ok((input, (date, time)))
}

fn in_time(input: &str) -> IResult<&str, Duration> {
    duration(input)
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PartialDate {
    pub day: u32,
    pub month: Option<u32>,
    pub year: Option<i32>,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TimeSpec {
    Duration(Duration),
    Time(NaiveTime),
    Date((PartialDate, NaiveTime)),
}

// at 08:32 arbitrarytext
// day 03 at 08:30 arbitrarytext
// day 03/08 at 8 arbitrarytext
// 4h arbitrarytext
// 4 h arbitrarytext
// 4 hours arbitrarytext
impl FromStr for TimeSpec {
    type Err = nom::error::Error<String>;
    fn from_str(a: &str) -> Result<Self, nom::error::Error<String>> {
        alt((
            map(d!(at_time), TimeSpec::Time),
            map(d!(on_day), TimeSpec::Date),
            map(d!(in_time), TimeSpec::Duration),
        ))(a)
        .finish()
        .map(|(_, r)| r)
        .map_err(|s| nom::error::Error {
            input: s.input.to_owned(),
            code: s.code,
        })
    }
}

#[cfg(test)]
mod test {
    use super::{PartialDate, TimeSpec};
    use chrono::{Duration, NaiveTime};

    use proptest::prelude::*;

    proptest! {
        #[test]
        fn doesnt_crash(s in ".*") {
            let _ = s.parse::<TimeSpec>();
        }

        #[test]
        fn valid_full_dates(s in "(day|dia) -?[0-9]+/-?[0-9]+/-?[0-9]+ (at|às|as|@) -?[0-9]+:-?[0-9]+:-?[0-9]") {
            match s.parse() {
                Err(_) => (),
                Ok(TimeSpec::Date((date, _))) if (0..31).contains(&date.day)
                    && (0..12).contains(&date.month.unwrap()) => {}
                Ok(o) => panic!("Invalid output {:?} for input {:?}", o, s),
            }
        }

        #[test]
        fn valid_times(s in "(at|às|as|@) -?[0-9]+:-?[0-9]+:-?[0-9]") {
            match s.parse() {
                Err(_) => (),
                Ok(TimeSpec::Time(_))  => (),
                Ok(o) => panic!("Invalid output {:?} for input {:?}", o, s),
            }
        }
    }

    #[test]
    fn huge_minute() {
        assert!("at 20:500".parse::<TimeSpec>().is_err());
    }

    #[test]
    fn full_day_parse() {
        assert_eq!(
            "day 04 at 8".parse(),
            Ok(TimeSpec::Date((
                PartialDate {
                    day: 4,
                    month: None,
                    year: None
                },
                NaiveTime::from_hms_opt(8, 0, 0).unwrap()
            )))
        )
    }

    #[test]
    fn full_day_parse1() {
        assert_eq!(
            "day 04/05 at 8:34".parse(),
            Ok(TimeSpec::Date((
                PartialDate {
                    day: 4,
                    month: Some(5),
                    year: None
                },
                NaiveTime::from_hms_opt(8, 34, 0).unwrap()
            )))
        )
    }

    #[test]
    fn small_time() {
        let r = "at 0:-0:0".parse::<TimeSpec>();
        assert!(r.is_err(), "{:?}", r);
    }

    macro_rules! make_test {
        ($($time:ident => $ctor:ident$(* $mult:expr)?),* $(,)?) => {
            paste::paste! {$(
                #[test]
                fn [<$ctor _from_ $time _no_space>]() {
                    let r = TimeSpec::Duration(Duration::$ctor(2 $(* $mult)?));
                    let x = concat!("2", stringify!($time));
                    let parsed = x.parse::<TimeSpec>().unwrap();
                    assert_eq!(parsed, r, "tried to parse {x:?}. Got {parsed:?}, expected: {r:?}");
                }

                #[test]
                fn [<$ctor _from_ $time _space>]() {
                    let r = TimeSpec::Duration(Duration::$ctor(2 $(* $mult)?));
                    let x = concat!("2 ", stringify!($time));
                    let parsed = x.parse::<TimeSpec>().unwrap();
                    assert_eq!(parsed, r, "tried to parse {x:?}. Got {parsed:?}, expected: {r:?}");
                }
            )*}
        }
    }

    make_test! {
        s => seconds,
        sec => seconds,
        secs => seconds,
        second => seconds,
        seconds => seconds,
        segundo => seconds,
        segundos => seconds,
        m => minutes,
        min => minutes,
        minute => minutes,
        minutes => minutes,
        minuto => minutes,
        minutos => minutes,
        h => hours,
        hour => hours,
        hours => hours,
        hora => hours,
        horas => hours,
        d => days,
        day => days,
        days => days,
        dia => days,
        dias => days,
        w => weeks,
        week => weeks,
        weeks => weeks,
        semana => weeks,
        semanas => weeks,
        month => days * 30,
        months => days * 30,
        mes => days * 30,
        meses => days * 30,
        year => days * 365,
        years => days * 365,
        ano => days * 365,
        anos => days * 365,
    }
}
