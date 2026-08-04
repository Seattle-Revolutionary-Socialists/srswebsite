import json
from time import gmtime,strftime
from copy import deepcopy
from datetime import date, timedelta
import sys

res = []

# TODO: move this to 
# next n weekdays as yyyy-mm-dd
def next_weekdays(day_name: str, n: int):
    start_date = date.today()
    weekday_map = {
        "monday": 0,
        "tuesday": 1,
        "wednesday": 2,
        "thursday": 3,
        "friday": 4,
        "saturday": 5,
        "sunday": 6,
    }

    target = weekday_map[day_name.lower()]
    days_until = (target - start_date.weekday()) % 7
    first = start_date + timedelta(days=days_until)
    return [
        (first + timedelta(weeks=i)).strftime("%Y-%m-%d")
        for i in range(n)
    ]

with open(f"./events/source-events-{sys.argv[1]}.json", "r") as f:
    data = json.load(f)
    for event in data['events']:
        # TODO: remove "flavored instances" of event (tuesday events)
        if 'date' in event and event['date'] >= strftime("%Y-%m-%d", gmtime()):
            res.append(event)
        elif 'recurring' in event and event['recurring']:
            for next_d in next_weekdays(event['dayOfWeek'], 3):
                e = deepcopy(event)
                e['date'] = next_d
                res.append(e)
    
    
with open("./events.json", "w") as f:
    json.dump({'events': sorted(res, key=lambda a: a['date'])}, f)
