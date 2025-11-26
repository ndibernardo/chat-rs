-- Create databases for microservices
SELECT 'CREATE DATABASE "user"' WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname = 'user')\gexec
SELECT 'CREATE DATABASE chat' WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname = 'chat')\gexec
