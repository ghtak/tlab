import 'just/db.just'
import 'just/app.just'
import 'just/http.just'

[windows]
set shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-Command"]

# 사용 가능한 명령 목록 표시
default:
    @just --list
