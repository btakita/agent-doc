# Import Commands

Import conversations from external sources into corky's conversation format.

## SMS Import
```bash
corky sync sms-import <FILE.xml>
```
Import SMS Backup & Restore XML files. Groups messages by phone number.

## Telegram Import
```bash
corky sync telegram-import <DIR>
```
Import Telegram Desktop JSON export directory.

## Slack Import
```bash
corky slack import <FILE.zip>
```
Import Slack workspace export archive.
