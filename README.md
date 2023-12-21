# towl
Central log collector

# Storing data

To manage log data we created a small filesystem like binary format. This format has 3 parts:
- Log Header
- Log Index
- Log Data

## Header

Contains a magic number, towl version number, meta data (organization name, file title, file ID). Based on the header data we can check towl files and select the right one during bulk process.

Header is serialized via bincode serializer.

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