mv log backup_log
cp database.json backup_database.json
pid=`pgrep leetcode_daily`
target/release/leetcode_daily > log &
if [[ $pid =~ ^[0-9]{5,}$ ]]
then
	echo Killing $pid
	kill $pid
fi
