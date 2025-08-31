#[doc(hidden)]
#[deprecated(note = "use in_files!")]
pub const FILES_DIR: &str = "files";
pub const NUMBERS: [&str; 11] = ["0⃣", "1⃣", "2⃣", "3⃣", "4⃣", "5⃣", "6⃣", "7⃣", "8⃣", "9⃣", "🔟"];

#[macro_export]
macro_rules! in_files {
    () => {
        #[expect(deprecated)]
        $crate::util::consts::FILES_DIR
    };
    ($($file:expr),*$(,)?) => {{
        #[expect(deprecated)]
        const fn f() -> &'static str {
            constcat::concat!($crate::util::consts::FILES_DIR, $("/", $file),*)
        }
        f()
    }};
}
