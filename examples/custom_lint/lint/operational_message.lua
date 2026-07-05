function register(lint)
  lint:rule({
    id = "operations/message-not-empty",
    title = "Operational message is empty",
    help = "Set a non-empty message before releasing the package.",
    target = "variable=operational_message",
    handler = "check_message",
  })
end

function check_message(package, variable)
  if variable.resolve.default == "" then
    return {
      {
        message = "operational_message default value must not be empty",
        path = "/resolve/default",
      }
    }
  end
  return {}
end
