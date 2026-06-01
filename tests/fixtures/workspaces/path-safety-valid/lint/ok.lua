function register(lint)
  lint:on({
    stage = "policy",
    entity = "workspace",
    rule = "path-safety/noop",
    handler = "check_workspace",
  })
end

function check_workspace(ctx)
  return {}
end
