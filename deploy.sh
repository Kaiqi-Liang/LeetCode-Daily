git pull
cargo b --release
pkill leetcode_daily
target/release/leetcode_daily > log &
