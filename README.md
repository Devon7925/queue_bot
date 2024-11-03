# Discord Queue Bot

A simple discord bot to matchmake players into games. 

## Project state
Currently to use this bot you will need to self host. It has also not undergone rigorous testing. In addition, many things are not configurable as I am currently primarily building it to fit my use case.

## Features
* Skill tracking
* Skill based matchmaking
* Roles for matchmaking
* Groups
* Ability to mark player as leaver/noshow
* Queue bans
* Lobby host tracking

Configurable parameters:
* Team size
* Team count
* Category for game channels to go into
* Voice channel for players to join queue
* Voice channel to move players to after game conclusion
* Maps & map voting
* Number of maps for a map vote
* Parameters for skill based matchmaking (configurable per player)
* Region based matchmaking(based on discord role)
* Roles players can queue with
* Valid role combinations for a queue

## Future plans

* Avoided players

## Communities using this bot

This bot is currently under testing by the Overwatch [6v6 Adjustments](https://github.com/6v6-Adjustments/6v6-adjustments) community discord.

## How to Run

* Set the `DISCORD_BOT_TOKEN` environment variable to your bot's token
    * If you don't have one you can get one via the discord developer portal
    * You also must invite your bot to your server
* Clone this repository
* Install cargo
* Execute `cargo run`

## How to setup bot for your discord server

Note: You will need the manage channels permission to run config commands

* Use `/create_queue` to generate a queue on your server
* Use `/configure` subcommands to change parameters
* Use `/create_register_message` to create a message that allows players to set their mmr
    * By default players can use this to set their mmr *at any time* which is likely not what you want.
    * This can also be configured to give a role via `/configure register_role`. This role in turn can be used to give access to queue channels and removes access from the register channel.
    * TODO: Document format
* Use `/create_queue_message` to create a message that allows people to join and leave queue
    * You can also use `/configure queue_channels` to set voice channels that queue people
    * Or players can queue with `/queue` and `/leave_queue`
* Use `/create_roles_message` to create a message that allows players to configure their queue roles
    * TODO: Document how to set up roles


