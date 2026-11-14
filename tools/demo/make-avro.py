"""Writes the sample Avro file used by the demo and the UI tests.

    python tools/demo/make-avro.py [codec] [output]

Needs fastavro (and cramjam for the snappy codec): pip install fastavro cramjam
"""
import datetime
import decimal
import sys
from pathlib import Path

import fastavro

SCHEMA = {
    "type": "record",
    "name": "Reading",
    "namespace": "station",
    "fields": [
        {"name": "sensor", "type": "string"},
        {"name": "kind", "type": {"type": "enum", "name": "Kind", "symbols": ["TEMPERATURE", "WIND"]}},
        {"name": "value", "type": "double"},
        {"name": "samples", "type": "int"},
        {"name": "taken", "type": {"type": "long", "logicalType": "timestamp-millis"}},
        {"name": "day", "type": {"type": "int", "logicalType": "date"}},
        {"name": "cost", "type": {"type": "bytes", "logicalType": "decimal", "precision": 8, "scale": 2}},
        {"name": "note", "type": ["null", "string"], "default": None},
        {"name": "tags", "type": {"type": "array", "items": "string"}},
        {"name": "limits", "type": {"type": "map", "values": "double"}},
    ],
}


def records(n=12):
    start = datetime.datetime(2026, 11, 1, 6, 0, tzinfo=datetime.timezone.utc)
    for i in range(n):
        wind = i % 3 == 2
        yield {
            "sensor": ["north", "south", "roof"][i % 3],
            "kind": "WIND" if wind else "TEMPERATURE",
            "value": round(4.5 + i * 0.75, 2) if wind else round(18.0 + i * 0.4, 2),
            "samples": 30 + i,
            "taken": start + datetime.timedelta(hours=i),
            "day": start.date() + datetime.timedelta(days=i // 6),
            "cost": decimal.Decimal("12.50") + decimal.Decimal(i) / 4,
            "note": "calibrated" if i % 4 == 0 else None,
            "tags": ["outdoor", "wind"] if wind else ["outdoor"],
            "limits": {"low": -20.0, "high": 45.0},
        }


def main():
    codec = sys.argv[1] if len(sys.argv) > 1 else "deflate"
    out = Path(sys.argv[2]) if len(sys.argv) > 2 else Path(__file__).parent / "weather-station" / "data" / "readings.avro"
    with open(out, "wb") as f:
        fastavro.writer(f, fastavro.parse_schema(SCHEMA), list(records()), codec=codec)
    print(f"{out}  {out.stat().st_size} bytes  codec={codec}")


if __name__ == "__main__":
    main()
