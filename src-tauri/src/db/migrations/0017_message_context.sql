-- Which context an assistant message was written in: 'public' (the
-- model saw the user's messages and public answers only) or 'private'
-- (it saw the whole chat, including private mail, calendar and application
-- data). NULL for messages from before this column: treated as private
-- once any earlier message read private data.
ALTER TABLE messages ADD COLUMN context TEXT;
