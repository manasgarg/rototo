function register(lint)
  lint:rule({
    id = "payments/max-token-budget",
    title = "Token budget exceeds payments policy",
    help = "Lower max_output_tokens or update the payments policy.",
    target = "/variables",
    handler = "reject_variable",
  })
end

function reject_variable(workspace, variable)
  if variable.id == "custom-valid" then
    return {
      {
        message = "custom lint rejected " .. variable.id
      }
    }
  end
  return {}
end
