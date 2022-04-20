# Cloudwatcher: Watch multiple AWS CloudWatch log groups

Given one or more log groups, this app will periodically retrieve the latest log events and display them. Only new log
events are displayed to avoid duplication in the output.

```
Usage: cloudwatcher [OPTIONS]

Optional arguments:
  -h, --help           print help message
  -r, --region REGION  override region

Available commands:
  list   list cloudwatch log groups
  watch  watch logs from cloudwatch log groups
```
