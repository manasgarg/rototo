function register(lint)
  lint:rule({
    id = "payments/max-token-budget",
    title = "Token budget exceeds payments policy",
    help = "Lower max_output_tokens or update the payments policy.",
    target = "/variables/agent-config",
    handler = "check_token_budget",
  })

  lint:rule({
    id = "payments/max-token-budget",
    title = "Token budget exceeds payments policy",
    help = "Lower max_output_tokens or update the payments policy.",
    target = "/variables/agent-config",
    handler = "missing_handler",
  })
end

function check_token_budget(package, variable)
  local default = variable.resolve and variable.resolve.default
  local budget = default
  if type(default) == "table" then
    budget = default[1]
  end
  if budget ~= nil and budget > 5000 then
    return {
      {
        message = variable.id .. ".default"
          .. " exceeds 5000 output tokens",
        path = "/resolve/default",
      }
    }
  end
  return {}
end
