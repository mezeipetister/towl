# towl
Central log collector

# How we store data

To store log data we have created a small filesystem like **binary format**. This format has 3 parts:
- First 1024 bytes: log header
- 1024-2048 bytes: log index
- after the first 2048 bytes: Log Data

Towl stores log entries in towl db files - using the binary format described above.

Working with towl we can syncrosnise our local towl clients - checking and downloading these towl binary data files; and work with them locally.

## Header

Contains a magic number, towl version number, meta data (organization name, file title, file ID). Based on the header data we can check towl files and select the right one during bulk process.

Header is serialized via bincode serializer.

| data field | description |
| --- | --- |
| magic | towl magic number |
| version | i32 |
| org | organization name, optional |
| title | title of file, optional |
| id | file id, we use it to identify towl file |

## Index

Contains the following data:

| Name | Description |
| --- | --- |
|Opened dtime | what time the log file was created|
|Closed dtim | What time the log file was closed |
|Count | how many entries it stores|
| First date | Received dtime of first stored log entry
| Last date | Received dtime of last stored log entry

Header is serialized via bincode serializer.

## Log data

After header we store all the log entries, serialized by bincode. All entries are appended to the file after each other - continuously.

In log data we store entries.

*Entry*

|Field|Description|
|---|---|
|sender|log collector ID|
|received|received dtime|
|log_format|log format encoded with number|
|log_entry|log message|

### Log format code table

|format code|format name|
|---|---|
|0|Free text|
|1|Systemctl json format|

## Data partitioning

For managing log entries we have 2 kind of data partition strategy. Store entries in towl file up to a maximum entry number e.g. 50_000 / file, or creating a file based on date or time, e.g. 1 file per day.

## Syncinc data

From local point of view we can grab remote data by a file ID, and/or count number. If we have a local copy of a data file with ID 3, and it contains 47_000 entries, but that file has 70_000 entries remotely, we can request a partial update by pointint ID:3, COUNT: 47_000. This request should pull the remaining 23_000 entries.

## Performance

Adding 50_000 entries takes ~ 1.74 secs [^1].\
Reading 50_000 entries takes ~ 0.58 secs [^1].

[^1]: Test machine: Macbook Air 2017, SSD

## Performance requirements

As Towl stores all the data in towl files, reading and writing is streamed, it has low memory need (maybe ~10 MB) to store and manage data. For log analysis it needs CPU cores, and for storing huge amount of logs it needs free disk space. Currently we have no real life data for log analysis, but it should easily handle a few millions of log entries.