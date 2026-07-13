function register(lint)
  lint:rule({
    id = "payments/max-token-budget",
    title = "Token budget exceeds payments policy",
    help = "Lower max_output_tokens or update the payments policy.",
    target = "variable=",
    handler = "fail_variable",
  })
end

function fail_variable(package, variable)
  if variable.id == "custom_failed" then
    error("script failed for " .. variable.id)
  end
  return {}
end
