# Reviewporter
This tool is designed to help you keep track of the pull requests that are awaiting review in your Azure repository by sending a private message to each developer on Slack. With this tool, you can make sure that your team members are aware of the pending pull requests and take action to review them promptly.


## Usage
### Notify developers about pull requests that are awaiting review
1. Create a configuration TOML file with the following structure and fill in the appropriate values for your specific use case:
```toml
[azure]
base_url = "The base URL of your Azure DevOps server (e.g., 'https://dev.azure.com/your-organization')"
token = "Your Personal Access Token (PAT) for Azure DevOps authentication"
project = "The name of the Azure DevOps project where the repositories are located"
team_name = "The name of the team whose members should receive notifications"
[slack]
token = "Your Slack bot token for authentication"
team_id = "The unique identifier (ID) of your Slack workspace"
usergroup_id = "The ID of the Slack usergroup containing the users who should receive notifications"
```
2. Run the program with the path to the configuration file as the only argument:
```
reviewporter --config <CONFIGFILE> send-reports -- <LIST OF AZURE REPOSITORIES>
```

The program will perform the following actions:
* Obtain all Azure DevOps teams in the provided project and find the team identifier by name.
* Check all provided repositories for all team users for unreviewed requests.
* Obtain Slack usergroup users and their profiles to get user names.
* Match Azure DevOps and Slack users using their names.
* Send private messages to users who have not reviewed Pull Requests.

---
### Add reviewers to active pull request
Revieporter can also be used to add reviewers to active pull request.
1. Create a configuration TOML file with the following structure and fill in the appropriate values for your specific use case:
```toml
[azure]
base_url = "The base URL of your Azure DevOps server (e.g., 'https://dev.azure.com/your-organization')"
token = "Your Personal Access Token (PAT) for Azure DevOps authentication"
project = "The name of the Azure DevOps project where the repositories are located"
team_name = "The name of the team whose members should receive notifications"

[azure.pull_request_reviewers]
required_reviewers_count = 2

[[azure.pull_request_reviewers.teams]]
name = "Dev team name"
required_reviewers_team = "Required reviewers team"

[slack]
token = "Your Slack bot token for authentication"
team_id = "The unique identifier (ID) of your Slack workspace"
usergroup_id = "The ID of the Slack usergroup containing the users who should receive notifications"
```
Each team is represented as a separate table in the configuration file. You can add multiple team definitions to accommodate different groups of developers.

Each team definition contains two properties: `name` and `required_reviewers_team`. The `name` is required property which specifies the name of the team, while the `required_reviewers_team property` optional property identifies the team that should act as the pool of required reviewers for pull requests.

The `required_reviewers_team` property designates a specific team whose members must be included as required reviewers for pull requests. This team and the developer's team act as the primary sources for the required reviewers, and they will be added in a round-robin technique. If `required_reviewers_team` property is not set for the team, then developer's team members will be added as required.

Configuration should specify an umbrella team under the `[azure]` section. This team, identified by the `team_name` property, represents a broader group of developers. All members of the umbrella team will be included as reviewers for pull requests.

Additionally, reviewporter integrates with the Slack API to check the availability of team members. It checks the vacation status of each developer by querying the Slack API. If a team member is on vacation, they will not be added as a required reviewer for the pull request.

2. Run the program with the path to the configuration file as the only argument:
```
reviewporter --config <CONFIGFILE> add-reviewers --repository="<REPO_ID> --request-id=<PR_ID>
```

## Building the project
To build the project using Rust, make sure you have Rust and Cargo (the Rust package manager) installed. If you haven't already, you can install both by following the instructions at https://www.rust-lang.org/tools/install.

Once Rust and Cargo are installed, you can build the project using the following command:
```
# Build the project (this will create an executable file in the 'target/release' directory)
cargo build --release
```
