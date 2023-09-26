#!/bin/sh
cd /home/laszlo/twitter-scraps-manager
docker exec postgres-bookmarks pg_dump -U postgres > backup/public.sql
docker exec postgres-bookmarks psql -U postgres -c "COPY (select * from tweets order by sort_index asc) to stdout with csv header delimiter ',';" > backup/tweets.csv
git add backup
git commit -m "backup database"
git push
