deploy:
	pkill leetcode_daily || true
	cargo b --release
	target/release/leetcode_daily &
