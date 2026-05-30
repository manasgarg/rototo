function lint_value(value)
  if value.value == "" then
    return {
      {
        rule = "consumer-experience/message-not-empty",
        message = "value " .. value.name .. " must not be empty"
      }
    }
  end
  return {}
end
