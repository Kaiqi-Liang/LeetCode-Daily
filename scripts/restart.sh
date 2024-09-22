pid=`pgrep leetcode_daily`
mv log old_log
target/release/leetcode_daily > log &
if [[ $pid =~ ^[0-9]{5,}$ ]]
then
	echo Killing $pid
	kill $pid
fi
