function register(lint)
  lint:on({
    stage = "policy",
    entity = "workspace",
    rule = {
          id = "path-safety/noop",
          title = "Path safety no-op",
          help = "No-op rule used to keep the path safety lint fixture registered.",
        },
    handler = "check_workspace",
  })
end

function check_workspace(ctx)
  return {}
end
