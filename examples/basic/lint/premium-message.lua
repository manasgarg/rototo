function register(lint)
  lint:rule({
    id = "consumer-experience/message-not-empty",
    title = "Directory-backed message is empty",
    help = "Set a non-empty message.",
    target = "/variables/premium-message",
    handler = "check_message",
  })
end

function check_message(package, variable)
  if variable.resolve.default == "" then
    return {
      {
        message = "premium-message default value must not be empty",
        path = "/resolve/default",
      }
    }
  end

  for _, rule in ipairs(variable.resolve.rules) do
    if rule.value == "" then
      return {
        {
          message = "premium-message rule value must not be empty",
          path = "/resolve",
        }
      }
    end
  end

  return {}
end
