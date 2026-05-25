# Sillybot

Sillybot is a private Discord bot installed in explicitly approved Discord guilds. Its initial user-facing purpose is to prove interaction handling and durable shared counting.

## Language

**Sillybot**:
The Discord bot application provided to approved guilds.
_Avoid_: public bot

**Approved guild**:
A Discord guild explicitly permitted to install and use Sillybot. Sillybot may be installed in more than one approved guild.
_Avoid_: home server, single server

**Application command**:
A slash command exposed by Sillybot to users in an approved guild.
_Avoid_: text command, prefix command

**Ping command** (`/ping`):
An application command that confirms Sillybot can receive and respond to an interaction.
_Avoid_: health check

**Count command** (`/count`):
An application command that increments and returns the global counter.
_Avoid_: guild count, user count

**Global counter**:
A durable count shared by all approved guilds and users of Sillybot. Each count command increments the same value.
_Avoid_: guild counter, per-user counter

## Flagged Ambiguities

**Server**:
Use **guild** for a Discord installation boundary. Use **host** for the machine running Sillybot, when that concept is needed.

## Example Dialogue

Developer: "Should `/count` show how often this guild used Sillybot?"

Domain expert: "No. The count command increments the global counter, so invocations from every approved guild contribute to the same value."

Developer: "Can an unapproved guild invoke an application command?"

Domain expert: "No. Sillybot is available only in approved guilds."
