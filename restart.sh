pid=`pgrep leetcode_daily`
target/release/leetcode_daily > log &
if [[ $pid =~ ^[0-9]{4,}$ ]]
then
	echo Killing $pid
	kill $pid
fi
