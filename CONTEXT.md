# Sillybot

Sillybot is a self-hosted Discord bot. Each instance operator chooses which Discord guilds may use their Sillybot instance; its initial user-facing purpose is to prove interaction handling and durable shared counting.

## Language

**Sillybot**:
The Discord bot software distributed for self-hosting.
_Avoid_: centrally hosted bot, public bot

**Sillybot instance**:
A separately operated installation of Sillybot with one Discord application identity and its own global counter. One instance may be used in more than one installed guild.
_Avoid_: shared Sillybot service

**Instance operator**:
The person or group that operates a Sillybot instance and decides which Discord guilds may use it.
_Avoid_: Sillybot administrator

**Installed guild**:
A Discord guild in which an instance operator has made their Sillybot instance available.
_Avoid_: approved guild, home server, single server

**Application command**:
A slash command exposed by a Sillybot instance to users in an installed guild. Initial application commands are not available through direct messages.
_Avoid_: text command, prefix command

**Ping command** (`/ping`):
An application command that confirms a Sillybot instance can receive and respond to an interaction.
_Avoid_: health check

**Count command** (`/count`):
An application command that increments its Sillybot instance's global counter and returns the new value visibly in the invoking channel.
_Avoid_: guild count, user count

**Global counter**:
A durable current value belonging to one Sillybot instance and shared by all installed guilds and users of that instance. It does not retain who incremented it or where an increment occurred.
_Avoid_: guild counter, per-user counter

**Moderation command** (`/ban`, `/kick`, `/timeout`):
An application command through which an authorized member of an installed guild asks a Sillybot instance to apply a Discord moderation action in that guild.
_Avoid_: admin command, bot punishment

**Moderation audit channel**:
An installed guild's optional configured Discord channel for visible records of successful moderation commands performed through its Sillybot instance.
_Avoid_: global audit log, instance log channel

## Flagged Ambiguities

**Instance**:
Use **Sillybot instance** for one operator-controlled installation. Use **Sillybot** for the distributed software.

**Bot identity**:
One Discord application identity belongs to one **Sillybot instance**. Do not use one bot identity for independently operated instances.

**Server**:
Use **guild** for a Discord installation boundary. Use **host** for the machine running Sillybot, when that concept is needed.

## Example Dialogue

Developer: "Should `/count` show how often this guild used this instance?"

Domain expert: "No. The count command increments that Sillybot instance's global counter, so invocations from every installed guild contribute to the same value."

Developer: "Can a guild see that this instance has been counted in another guild?"

Domain expert: "Yes. `/count` visibly returns the instance-wide global counter value in the channel where it is invoked."

Developer: "Can the operator look up who performed earlier increments?"

Domain expert: "No. The global counter records its current value, not an invocation history."

Developer: "Does the Sillybot project decide which guilds may use an instance?"

Domain expert: "No. Each instance operator chooses where their Sillybot instance is made available."

Developer: "Can two separately stored counters run under the same Discord bot identity?"

Domain expert: "No. One Discord bot identity belongs to one Sillybot instance and one global counter."

Developer: "Can I use `/count` in a direct message to the bot?"

Domain expert: "No. Initial application commands are invoked in an installed guild."

Developer: "If I configure a moderation audit channel, does it collect records from every installed guild?"

Domain expert: "No. A moderation audit channel belongs to one installed guild and records successful moderation commands from that guild."
