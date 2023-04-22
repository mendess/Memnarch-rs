use crate::util::tuple_map::TupleMap;
use chrono::{Duration, NaiveTime};
use lazy_static::lazy_static;
use nom::{
    branch::alt,
    bytes::complete::{self as bytes, tag},
    character::complete::{self as character, space0 as spc},
    combinator::map,
    error::{make_error, ErrorKind},
    sequence::{delimited, pair, preceded, terminated, tuple},
    Finish, IResult,
};
use nom_regex::str::re_find;
use regex::Regex;
use std::ops::RangeBounds;

fn day_tag(s: &str) -> IResult<&str, &str> {
    alt((tag("day"), tag("dia")))(s)
}

fn at_tag(s: &str) -> IResult<&str, &str> {
    alt((tag("at"), tag("@"), tag("as"), tag("às")))(s)
}

fn spaced<'s, P, O, E>(p: P) -> impl FnMut(&'s str) -> IResult<&str, O, E>
where
    P: nom::Parser<&'s str, O, E>,
    E: nom::error::ParseError<&'s str>,
{
    delimited(spc, p, spc)
}

fn rest_trimed(s: &str) -> IResult<&str, &str> {
    preceded(spc, bytes::take_while1(|_| true))(s)
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
) -> impl FnMut(&'s str) -> IResult<&str, Option<u16>> {
    move |input| match alt((preceded(tag(tag_str), character::digit1), tag(" ")))(input)? {
        (_, " ") => Ok((input, None)),
        (a, digit) => match digit.parse() {
            Ok(i) if range.contains(&i) => Ok((a, Some(i))),
            _ => Err(nom::Err::Error(make_error(input, ErrorKind::Satisfy))),
        },
    }
}

fn date(input: &str) -> IResult<&str, PartialDate> {
    let (input, day) = parse_number(1..=31)(input)?.map_snd(u32::from);
    let (input, month) = preceded_number("/", 1..=12)(input)?.map_snd(|o| o.map(u32::from));
    let (input, year) = preceded_number("/", 0..)(input)?.map_snd(|o| o.map(i32::from));

    Ok((input, PartialDate { day, month, year }))
}

fn time(input: &str) -> IResult<&str, NaiveTime> {
    let (input, hour) = parse_number(0..24)(input)?;
    let (input, min) = preceded_number(":", 0..60)(input)?.map_snd(Option::unwrap_or_default);
    let (input, sec) = preceded_number(":", 0..60)(input)?.map_snd(Option::unwrap_or_default);
    Ok((
        input,
        NaiveTime::from_hms_opt(u32::from(hour), u32::from(min), u32::from(sec)).unwrap(),
    ))
}

fn duration(input: &str) -> IResult<&str, Duration> {
    lazy_static! {
        static ref SECONDS: Regex = Regex::new("^(s|sec|secs|seconds?|segundos?) ").unwrap();
        static ref MINUTES: Regex = Regex::new("^(m|min|mins|minutes?|minutos?) ").unwrap();
        static ref HOURS: Regex = Regex::new("^(h|hours?|horas?) ").unwrap();
        static ref DAYS: Regex = Regex::new("^(d|days?|dias?) ").unwrap();
        static ref WEEKS: Regex = Regex::new("^(w|weeks?|semanas?) ").unwrap();
        static ref MONTHS: Regex = Regex::new("^(months?|mes(es)?) ").unwrap();
        static ref YEARS: Regex = Regex::new("^(y|years?|anos?) ").unwrap();
    };
    let re = |r: &Regex| re_find(r.clone());

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

fn at_time(input: &str) -> IResult<&str, (NaiveTime, &str)> {
    pair(preceded(at_tag, spaced(time)), rest_trimed)(input)
}

fn on_day(input: &str) -> IResult<&str, ((PartialDate, NaiveTime), &str)> {
    let (input, (date, time, text)) = tuple((
        delimited(day_tag, spaced(date), at_tag),
        spaced(time),
        rest_trimed,
    ))(input)?;

    Ok((input, ((date, time), text)))
}

fn in_time(input: &str) -> IResult<&str, (Duration, &str)> {
    pair(duration, rest_trimed)(input)
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

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Reminder<'text> {
    pub text: &'text str,
    pub when: TimeSpec,
}

// at 08:32 arbitrarytext
// day 03 at 08:30 arbitrarytext
// day 03/08 at 8 arbitrarytext
// 4h arbitrarytext
// 4 h arbitrarytext
// 4 hours arbitrarytext

pub fn parse(a: &str) -> Result<Reminder<'_>, nom::error::Error<&str>> {
    alt((
        map(at_time, |(d, text)| Reminder {
            text,
            when: TimeSpec::Time(d),
        }),
        map(on_day, |(d, text)| Reminder {
            text,
            when: TimeSpec::Date(d),
        }),
        map(in_time, |(d, text)| Reminder {
            text,
            when: TimeSpec::Duration(d),
        }),
    ))(a)
    .finish()
    .map(|(_, r)| r)
}

#[cfg(test)]
mod test {
    use super::{parse, PartialDate, Reminder, TimeSpec};
    use chrono::{Duration, NaiveTime};

    use proptest::prelude::*;

    proptest! {
        #[test]
        fn doesnt_crash(s in ".*") {
            let _ = parse(&s);
        }

        #[test]
        fn valid_full_dates(s in "(day|dia) -?[0-9]+/-?[0-9]+/-?[0-9]+ (at|às|as|@) -?[0-9]+:-?[0-9]+:-?[0-9] cenas") {
            match parse(&s) {
                Err(_) => (),
                Ok(Reminder {
                    text,
                    when: TimeSpec::Date((date, _)),
                }) if (0..31).contains(&date.day)
                    && (0..12).contains(&date.month.unwrap())
                    && text == "cenas" => {}
                Ok(o) => panic!("Invalid output {:?} for input {:?}", o, s),
            }
        }

        #[test]
        fn valid_times(s in "(at|às|as|@) -?[0-9]+:-?[0-9]+:-?[0-9] cenas") {
            match parse(&s) {
                Err(_) => (),
                Ok(Reminder { text, when: TimeSpec::Time(_), }) if text == "cenas" => (),
                Ok(o) => panic!("Invalid output {:?} for input {:?}", o, s),
            }
        }
    }

    #[test]
    fn huge_minute() {
        assert!(parse("at 20:500 cenas").is_err());
    }

    #[test]
    #[should_panic]
    fn empty_reminder() {
        parse("at 20:40").unwrap();
    }

    #[test]
    #[should_panic]
    fn reminder_is_spaces() {
        parse("at 8 ").unwrap();
    }

    #[test]
    fn full_day_parse() {
        assert_eq!(
            parse("day 04 at 8 cenas"),
            Ok(Reminder {
                when: TimeSpec::Date((
                    PartialDate {
                        day: 4,
                        month: None,
                        year: None
                    },
                    NaiveTime::from_hms_opt(8, 0, 0).unwrap()
                )),
                text: "cenas"
            })
        )
    }

    #[test]
    fn full_day_parse1() {
        assert_eq!(
            parse("day 04/05 at 8:34 cenas"),
            Ok(Reminder {
                when: TimeSpec::Date((
                    PartialDate {
                        day: 4,
                        month: Some(5),
                        year: None
                    },
                    NaiveTime::from_hms_opt(8, 34, 0).unwrap()
                )),
                text: "cenas"
            })
        )
    }

    #[test]
    fn small_time() {
        let r = parse("at 0:-0:0 cenas");
        assert!(r.is_err(), "{:?}", r);
    }

    macro_rules! make_test {
        ($($time:ident => $ctor:ident$(* $mult:expr)?),* $(,)?) => {
            paste::paste! {$(
                #[test]
                fn [<$ctor _from_ $time _no_space>]() {
                    let r = Reminder {
                        text: "cenas",
                        when: TimeSpec::Duration(Duration::$ctor(2 $(* $mult)?))
                    };
                    let x = concat!("2", stringify!($time), " cenas");
                    eprintln!("{:?}", x);
                    assert_eq!(
                        parse(x).unwrap(),
                        r,
                    );
                    assert_eq!(
                        parse(concat!("2", stringify!($time), " cenas")).unwrap(),
                        r
                    );
                }

                #[test]
                fn [<$ctor _from_ $time _space>]() {
                    let r = Reminder {
                        text: "cenas",
                        when: TimeSpec::Duration(Duration::$ctor(2 $(* $mult)?))
                    };
                    assert_eq!(
                        parse(concat!("2 ", stringify!($time), " cenas")).unwrap(),
                        r,
                    );
                    assert_eq!(
                        parse(concat!("2 ", stringify!($time), " cenas")).unwrap(),
                        r,
                    );
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
