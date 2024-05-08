deploy:
	pkill leetcode_daily
	cargo b --release
	/target/release/leetcode_daily &