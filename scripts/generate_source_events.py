import json
from copy import deepcopy
from datetime import date, timedelta
import sys
from pathlib import Path

res = []

# TODO: move this to 
# next n weekdays as yyyy-mm-dd
def next_weekdays(day_name: str, n: int = 3, interval_weeks: int = 1, start_date: str = None, end_date: str = None):
    today = date.today()
    weekday_map = {
        "monday": 0,
        "tuesday": 1,
        "wednesday": 2,
        "thursday": 3,
        "friday": 4,
        "saturday": 5,
        "sunday": 6,
    }

    step = timedelta(weeks=max(1, interval_weeks))
    if start_date:
        first = date.fromisoformat(start_date)
        while first < today:
            first += step
    else:
        target = weekday_map[day_name.lower()]
        days_until = (target - today.weekday()) % 7
        first = today + timedelta(days=days_until)

    last = date.fromisoformat(end_date) if end_date else None

    res = []
    current = first
    while last is None or current <= last:
        res.append(current.strftime("%Y-%m-%d"))
        if last is None and len(res) >= n:
            break
        current += step

    return res

branch = sys.argv[1]
for path in Path(f"./events/{branch}").iterdir():
    with open(path, "r") as f:
        data = json.load(f)
        for event in data['events']:
            # TODO: remove "flavored instances" of event (tuesday events)
            if 'date' in event and event['date'] >= date.today().isoformat():
                res.append(event)
            elif 'recurring' in event and event['recurring']:
                for next_d in next_weekdays(event['dayOfWeek'], 3, event.get('intervalWeeks', 1), event.get('startDate'), event.get('endDate')):
                    e = deepcopy(event)
                    e['date'] = next_d
                    res.append(e)
    
    
with open("./events.json", "w") as f:
    json.dump({'events': sorted(res, key=lambda a: a['date'])}, f)
