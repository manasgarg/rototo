function register(lint)
  lint:rule({
    id = "operations/message-not-empty",
    title = "Operational message is empty",
    help = "Set a non-empty message before releasing the workspace.",
    target = "/variables/operational-message",
    handler = "check_message",
  })
end

function check_message(workspace, variable)
  if variable.resolve.default == "" then
    return {
      {
        message = "operational-message default value must not be empty",
        path = "/resolve/default",
      }
    }
  end
  return {}
end
