# Reviewporter
This tool is designed to help you keep track of the pull requests that are awaiting review in your Azure repository by sending a private message to each developer on Slack. With this tool, you can make sure that your team members are aware of the pending pull requests and take action to review them promptly.


## Usage
1. Create a configuration TOML file with the following structure and fill in the appropriate values for your specific use case:
```toml
[azure]
base_url = "The base URL of your Azure DevOps server (e.g., 'https://dev.azure.com/your-organization')"
token = "Your Personal Access Token (PAT) for Azure DevOps authentication"
project = "The name of the Azure DevOps project where the repositories are located"
team_name = "The name of the team whose members should receive notifications"
repositories = ["repo1", "repo2"]  # Array of repository names to check for unreviewed requests
[slack]
token = "Your Slack bot token for authentication"
team_id = "The unique identifier (ID) of your Slack workspace"
usergroup_id = "The ID of the Slack usergroup containing the users who should receive notifications"
```
2. Run the program with the path to the configuration file as the only argument:
```
reviewporter <path-to-configuration-file>
```

The program will perform the following actions:
* Obtain all Azure DevOps teams in the provided project and find the team identifier by name.
* Check all provided repositories for all team users for unreviewed requests.
* Obtain Slack usergroup users and their profiles to get user names.
* Match Azure DevOps and Slack users using their names.
* Send private messages to users who have not reviewed Pull Requests.

## Building the project
To build the project using Rust, make sure you have Rust and Cargo (the Rust package manager) installed. If you haven't already, you can install both by following the instructions at https://www.rust-lang.org/tools/install.

Once Rust and Cargo are installed, you can build the project using the following command:
```
# Build the project (this will create an executable file in the 'target/release' directory)
cargo build --release
```