"""
Convert UTC time in the log files to local time
"""
from datetime import datetime, timezone

utc_time = input("Enter UTC time in the format of yyyy-mm-dd HH:MM:SS\n")
date_time = datetime.strptime(utc_time, "%Y-%m-%d %H:%M:%S")
local_time = date_time.replace(tzinfo=timezone.utc).astimezone()
print(local_time.strftime("%d/%m/%Y %I:%M:%S%p"))
