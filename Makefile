deploy:
	pkill leetcode_daily || true
	git pull
	cargo b --release
	target/release/leetcode_daily > log &
